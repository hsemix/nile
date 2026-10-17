//! Independently buildable SQLite consumer of the public SDK; no engine dependencies.
use nile_extension_sdk::*;
use rusqlite::{
    limits::Limit,
    types::{Value as SqlValue, ValueRef},
    Connection, OpenFlags,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{sync_channel, SyncSender},
    Arc, Mutex,
};
use std::thread::JoinHandle;

// Response storage (JSON including escaping/structure plus binary buffers).
const RESULT_BYTES: usize = 900_000;
// SQLite's own per-value/encoded-row ceiling, independent of response storage.
const SQLITE_VALUE_BYTES: i32 = 4 * 1024 * 1024;
const SAFE: i64 = 9_007_199_254_740_991;
const REQUIRED: u64 = NILE_CAP_ASYNC | NILE_CAP_BUFFERS | NILE_CAP_RESOURCES | NILE_CAP_CANCEL;
type Error = (String, String);
type Result<T> = std::result::Result<T, Error>;
fn error(code: &str, message: impl ToString) -> Error {
    (code.into(), message.to_string())
}
fn sql_error(e: rusqlite::Error) -> Error {
    let code = match &e {
        rusqlite::Error::SqliteFailure(e, _) if e.code == rusqlite::ErrorCode::TooBig => {
            return error("SQLITE_RESULT_LIMIT", format!("SQLite value/row exceeds connection length limit ({SQLITE_VALUE_BYTES} bytes): {e}"));
        }
        rusqlite::Error::SqliteFailure(e, _) => format!("SQLITE_{}", e.extended_code),
        rusqlite::Error::MultipleStatement => "SQLITE_MULTIPLE_STATEMENTS".into(),
        _ => "SQLITE_STATEMENT".into(),
    };
    (code, e.to_string())
}
fn result_limit() -> Error {
    error(
        "SQLITE_RESULT_LIMIT",
        "query result exceeds bounded packet size; use LIMIT/pagination",
    )
}
#[derive(Default)]
struct ResultBudget {
    bytes: usize,
}
impl ResultBudget {
    fn checked_size(&self, additional: usize) -> Result<usize> {
        self.bytes
            .checked_add(additional)
            .filter(|&size| size <= RESULT_BYTES)
            .ok_or_else(result_limit)
    }
    fn reserve(&mut self, additional: usize) -> Result<()> {
        self.bytes = self.checked_size(additional)?;
        Ok(())
    }
    fn string(&mut self, value: &str) -> Result<()> {
        // serde_json writes borrowed chunks and escape sequences into the counter,
        // so even heavily escaped input needs no temporary encoded String.
        serde_json::to_writer(self, value).map_err(|_| result_limit())
    }
    fn value(&mut self, value: &Value) -> Result<()> {
        serde_json::to_writer(self, value).map_err(|_| result_limit())
    }
}
impl std::io::Write for ResultBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.reserve(bytes.len())
            .map_err(|_| std::io::Error::other("SQLite result limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encode_result(value: &Value, buffers: &[Vec<u8>]) -> Result<String> {
    let json = serde_json::to_string(value).map_err(|_| result_limit())?;
    // Final serialized packet validation includes all JSON structure/escaping.
    // Query construction already enforced this budget before allocating cells.
    let mut budget = ResultBudget::default();
    budget.reserve(json.len())?;
    for buffer in buffers {
        budget.reserve(buffer.len())?;
    }
    Ok(json)
}
struct Work {
    context: u64,
    call: u64,
    operation: String,
    args: Value,
    buffers: Vec<Vec<u8>>,
    base: PathBuf,
    cancelled: Arc<AtomicBool>,
}
struct State {
    sender: Mutex<Option<SyncSender<Work>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    stopping: Arc<AtomicBool>,
    calls: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
}
unsafe extern "C" fn resource_drop(_: u64) { /* Connections are owned and closed by the ordered worker. */
}
fn param(value: &Value, buffers: &[Vec<u8>]) -> Result<SqlValue> {
    Ok(match value {
        Value::Null => SqlValue::Null,
        Value::Bool(v) => SqlValue::Integer(i64::from(*v)),
        Value::Number(v) => {
            if let Some(i) = v.as_i64() {
                if !(-SAFE..=SAFE).contains(&i) {
                    return Err(error(
                        "SQLITE_INTEGER",
                        "use BigInt for integers outside the JS safe range",
                    ));
                }
                SqlValue::Integer(i)
            } else {
                let number = v
                    .as_f64()
                    .ok_or_else(|| error("SQLITE_PARAMETER", "invalid number"))?;
                if number.fract() == 0.0 && number.abs() > SAFE as f64 {
                    return Err(error(
                        "SQLITE_INTEGER",
                        "use BigInt for integers outside the JS safe range",
                    ));
                }
                SqlValue::Real(number)
            }
        }
        Value::String(v) => SqlValue::Text(v.clone()),
        Value::Object(v) if v.len() == 1 && v.contains_key("$i64") => {
            let text = v["$i64"]
                .as_str()
                .ok_or_else(|| error("SQLITE_INTEGER", "invalid $i64"))?;
            SqlValue::Integer(
                text.parse()
                    .map_err(|_| error("SQLITE_INTEGER", "integer outside signed 64-bit range"))?,
            )
        }
        Value::Object(v) if v.len() == 1 && v.contains_key("$blob") => {
            let i = v["$blob"]
                .as_u64()
                .ok_or_else(|| error("SQLITE_BLOB", "invalid buffer index"))?
                as usize;
            SqlValue::Blob(
                buffers
                    .get(i)
                    .ok_or_else(|| error("SQLITE_BLOB", "buffer index out of range"))?
                    .clone(),
            )
        }
        _ => return Err(error("SQLITE_PARAMETER", "unsupported bound parameter")),
    })
}
fn integer(i: i64) -> Value {
    if (-SAFE..=SAFE).contains(&i) {
        json!(i)
    } else {
        json!({"$i64": i.to_string()})
    }
}
fn run(
    work: &Work,
    client: Client,
    dbs: &mut HashMap<u64, Connection>,
    stopping: &Arc<AtomicBool>,
) -> Result<(Value, Vec<Vec<u8>>)> {
    if work.cancelled.load(Ordering::Acquire) || stopping.load(Ordering::Acquire) {
        return Err(error(
            "SQLITE_CANCELLED",
            "operation cancelled before execution",
        ));
    }
    if work.operation == "sqlite.open" {
        let path = work.args["path"]
            .as_str()
            .ok_or_else(|| error("SQLITE_PATH", "path must be a string"))?;
        if path.is_empty() || path.starts_with("file:") {
            return Err(error(
                "SQLITE_PATH",
                "empty paths and SQLite URI paths are unsupported",
            ));
        }
        let local = std::path::Path::new(path);
        if cfg!(windows)
            && matches!(
                local.components().next(),
                Some(std::path::Component::Prefix(_))
            )
            && !local.is_absolute()
        {
            return Err(error(
                "SQLITE_PATH",
                "drive-relative Windows paths are unsupported; use an absolute path",
            ));
        }
        let resolved = if path == ":memory:" {
            PathBuf::from(path)
        } else {
            work.base.join(path)
        };
        let read_only = work.args["options"]["readOnly"].as_bool().unwrap_or(false);
        let flags = if read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        };
        let conn = Connection::open_with_flags(resolved, flags).map_err(sql_error)?;
        conn.set_limit(Limit::SQLITE_LIMIT_LENGTH, SQLITE_VALUE_BYTES)
            .map_err(sql_error)?;
        let timeout = work.args["options"]["busyTimeoutMs"]
            .as_u64()
            .unwrap_or(1000);
        if timeout > 5000 {
            return Err(error(
                "SQLITE_OPTIONS",
                "busyTimeoutMs must be between 0 and 5000",
            ));
        }
        conn.busy_timeout(std::time::Duration::from_millis(timeout))
            .map_err(sql_error)?;
        let mut handle = 0;
        let status = unsafe {
            (client.0.resource_create.unwrap())(
                client.0.host,
                work.context,
                0,
                resource_drop,
                &mut handle,
            )
        };
        if status != NILE_OK {
            return Err(error(
                "SQLITE_RESOURCE",
                format!("cannot allocate resource ({status})"),
            ));
        }
        dbs.insert(handle, conn);
        return Ok((json!({"handle": handle.to_string()}), vec![]));
    }
    let handle = work.args["handle"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| error("SQLITE_HANDLE", "invalid connection handle"))?;
    let mut payload = 0;
    let valid = unsafe {
        (client.0.resource_get.unwrap())(client.0.host, work.context, handle, &mut payload)
    } == NILE_OK;
    if !valid || !dbs.contains_key(&handle) {
        return Err(error(
            "SQLITE_CLOSED",
            "connection is closed or belongs to another context",
        ));
    }
    if work.operation == "sqlite.close" {
        dbs.remove(&handle); // Drop on owning worker; rolls back open transactions.
        unsafe {
            (client.0.resource_release.unwrap())(client.0.host, work.context, handle);
        }
        return Ok((Value::Null, vec![]));
    }
    let conn = &dbs[&handle];
    let cancelled = work.cancelled.clone();
    let stopping = stopping.clone();
    conn.progress_handler(
        1000,
        Some(move || cancelled.load(Ordering::Acquire) || stopping.load(Ordering::Acquire)),
    )
    .map_err(sql_error)?;
    let sql = work.args["sql"]
        .as_str()
        .ok_or_else(|| error("SQLITE_STATEMENT", "sql must be a string"))?;
    let mut statement = conn.prepare(sql).map_err(sql_error)?; // rusqlite rejects nonempty tails.
    let params = work.args["params"]
        .as_array()
        .ok_or_else(|| error("SQLITE_PARAMETER", "params must be an array"))?;
    let values = params
        .iter()
        .map(|v| param(v, &work.buffers))
        .collect::<Result<Vec<_>>>()?;
    if work.operation == "sqlite.execute" {
        let changes = statement
            .execute(rusqlite::params_from_iter(values))
            .map_err(sql_error)?;
        return Ok((
            json!({"changes": changes, "lastInsertRowId": integer(conn.last_insert_rowid())}),
            vec![],
        ));
    }
    // Check borrowed column names too before owning them. Count one row's keys;
    // each actual row is accounted separately below.
    let mut key_budget = ResultBudget::default();
    let mut names = Vec::new();
    for name in statement.column_names() {
        key_budget.string(name)?;
        key_budget.reserve(2)?; // colon/comma
        names.push(name.to_string());
    }
    let unique: HashSet<_> = names.iter().collect();
    if unique.len() != names.len() {
        return Err(error(
            "SQLITE_DUPLICATE_COLUMN",
            "duplicate column names; use SQL AS aliases",
        ));
    }
    let mut cursor = statement
        .query(rusqlite::params_from_iter(values))
        .map_err(sql_error)?;
    let mut rows = Vec::new();
    let mut buffers = Vec::new();
    let mut budget = ResultBudget::default();
    budget.reserve(2)?; // outer array brackets
    while let Some(row) = cursor.next().map_err(sql_error)? {
        if rows.len() >= 10_000 {
            return Err(error(
                "SQLITE_RESULT_LIMIT",
                "query exceeds 10000 rows; use LIMIT/pagination",
            ));
        }
        budget.reserve(2 + usize::from(!rows.is_empty()))?; // braces and row separator
        let mut object = serde_json::Map::new();
        for (i, name) in names.iter().enumerate() {
            budget.string(name)?;
            budget.reserve(1 + usize::from(i != 0))?; // colon and column separator
            let value = match row.get_ref(i).map_err(sql_error)? {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(i) => integer(i),
                ValueRef::Real(f) => {
                    if !f.is_finite() {
                        return Err(error(
                            "SQLITE_NUMBER",
                            "non-finite SQL REAL cannot be represented in JSON",
                        ));
                    }
                    json!(f)
                }
                ValueRef::Text(t) => {
                    budget.checked_size(t.len())?; // borrowed length before UTF-8 validation/copy
                    let text = std::str::from_utf8(t)
                        .map_err(|_| error("SQLITE_UTF8", "SQLite text is not UTF-8"))?;
                    budget.string(text)?; // exact escaped bytes before owning a copy
                    object.insert(name.clone(), Value::String(text.to_owned()));
                    continue;
                }
                ValueRef::Blob(b) => {
                    if buffers.len() >= 16 {
                        return Err(error(
                            "SQLITE_RESULT_LIMIT",
                            "query exceeds 16 binary buffers",
                        ));
                    }
                    let index = buffers.len();
                    budget.reserve(b.len())?; // borrowed length before copying
                    let tag = json!({"$blob": index});
                    budget.value(&tag)?;
                    buffers.push(b.to_vec());
                    object.insert(name.clone(), tag);
                    continue;
                }
            };
            budget.value(&value)?;
            object.insert(name.clone(), value);
        }
        rows.push(Value::Object(object));
    }
    Ok((Value::Array(rows), buffers))
}
unsafe extern "C" fn dispatch(user: u64, request: *const NileRequest) -> NileStatus {
    guard(|| {
        if request.is_null() {
            return NILE_INVALID;
        }
        let state = &*(user as *const State);
        let r = &*request;
        if r.flags != 0 || state.stopping.load(Ordering::Acquire) {
            return NILE_CLOSED;
        }
        let args = match serde_json::from_slice(r.json.bytes()) {
            Ok(v) => v,
            Err(_) => return NILE_INVALID,
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let work = Work {
            context: r.context,
            call: r.call,
            operation: String::from_utf8_lossy(r.operation.bytes()).into_owned(),
            args,
            buffers: r
                .buffers
                .slices()
                .iter()
                .map(|b| b.bytes().to_vec())
                .collect(),
            base: PathBuf::from(String::from_utf8_lossy(r.base_path.bytes()).into_owned()),
            cancelled: cancelled.clone(),
        };
        state.calls.lock().unwrap().insert(r.call, cancelled);
        let status = match state.sender.lock().unwrap().as_ref() {
            Some(sender) => match sender.try_send(work) {
                Ok(()) => NILE_OK,
                Err(_) => NILE_OVERLOADED,
            },
            None => NILE_CLOSED,
        };
        if status != NILE_OK {
            state.calls.lock().unwrap().remove(&r.call);
        }
        status
    })
}
unsafe extern "C" fn cancel(user: u64, _: u64, call: u64) {
    guard(|| {
        let state = &*(user as *const State);
        if let Some(flag) = state.calls.lock().unwrap().get(&call) {
            flag.store(true, Ordering::Release);
        }
        NILE_OK
    });
}
unsafe extern "C" fn dispose(user: u64, _: u64) -> NileStatus {
    guard(|| {
        let state = &*(user as *const State);
        state.stopping.store(true, Ordering::Release);
        state.sender.lock().unwrap().take();
        if let Some(worker) = state.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
        state.calls.lock().unwrap().clear();
        NILE_OK
    })
}
unsafe extern "C" fn shutdown(user: u64) -> NileStatus {
    guard(|| {
        let status = dispose(user, 0);
        if status != NILE_OK {
            return status;
        }
        drop(Box::from_raw(user as *mut State));
        NILE_OK
    })
}

/// Initialize one extension instance from the versioned public host table.
///
/// # Safety
/// `api` must point to a readable ABI table and `out` to writable storage for a
/// full `NileExtension`. The host must obey the SDK callback/lifetime contract.
#[no_mangle]
pub unsafe extern "C" fn nile_extension_init(
    api: *const NileExtensionApi,
    out: *mut NileExtension,
) -> NileStatus {
    guard(|| {
        if out.is_null() {
            return NILE_INVALID;
        }
        let client = match Client::from_api(api) {
            Ok(c) => c,
            Err(e) => return e,
        };
        if client.0.capabilities & REQUIRED != REQUIRED {
            return NILE_INVALID;
        }
        let (sender, receiver) = sync_channel::<Work>(128);
        let stopping = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(Mutex::new(HashMap::new()));
        let worker_stopping = stopping.clone();
        let worker_calls = calls.clone();
        let state = Box::new(State {
            sender: Mutex::new(Some(sender)),
            worker: Mutex::new(None),
            stopping,
            calls,
        });
        let user = Box::into_raw(state) as u64;
        *out = NileExtension {
            required_capabilities: REQUIRED,
            user,
            context_dispose: Some(dispose),
            cancel: Some(cancel),
            shutdown: Some(shutdown),
            ..NileExtension::default()
        };
        let worker = match std::thread::Builder::new()
            .name("nile-sqlite".into())
            .spawn(move || {
                let mut dbs = HashMap::new();
                while let Ok(work) = receiver.recv() {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let (value, buffers) = run(&work, client, &mut dbs, &worker_stopping)?;
                        let json = encode_result(&value, &buffers)?;
                        Ok((json, buffers))
                    }))
                    .unwrap_or_else(|_| Err(error("SQLITE_PANIC", "SQLite worker panicked")));
                    match result {
                        Ok((json, buffers)) => {
                            let status =
                                client.reply(work.context, work.call, Ok(&json), &buffers, false);
                            if status == NILE_OVERLOADED {
                                client.reply(
                                    work.context,
                                    work.call,
                                    Err((
                                        "SQLITE_RESULT_LIMIT",
                                        "encoded result exceeds packet limit",
                                    )),
                                    &[],
                                    false,
                                );
                            }
                        }
                        Err((code, message)) => {
                            client.reply(
                                work.context,
                                work.call,
                                Err((&code, &message)),
                                &[],
                                false,
                            );
                        }
                    }
                    worker_calls.lock().unwrap().remove(&work.call);
                }
                // Connections and outstanding transactions close on this thread.
            }) {
            Ok(w) => w,
            Err(_) => return NILE_FAILED,
        };
        *(&*(user as *const State)).worker.lock().unwrap() = Some(worker);
        for name in [
            "sqlite.open",
            "sqlite.execute",
            "sqlite.query",
            "sqlite.close",
        ] {
            let status = (client.0.register_operation.unwrap())(
                client.0.host,
                NileSlice::new(name.as_bytes()),
                dispatch,
                user,
            );
            if status != NILE_OK {
                return status;
            }
        }
        NILE_OK
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn result_budget_counts_escaping_and_rejects_overflow_without_allocating() {
        for text in ["", "plain", "\"\\\n\r\t\0\u{1f}🦀"] {
            let mut budget = ResultBudget::default();
            budget.string(text).unwrap();
            assert_eq!(budget.bytes, serde_json::to_string(text).unwrap().len());
        }
        let mut budget = ResultBudget::default();
        budget.reserve(RESULT_BYTES - 1).unwrap();
        assert!(budget.reserve(2).is_err());
        assert!(budget.reserve(usize::MAX).is_err());
        assert_eq!(budget.bytes, RESULT_BYTES - 1);
        budget.reserve(1).unwrap();
        assert_eq!(budget.bytes, RESULT_BYTES);
        assert!(budget.string("").is_err());
    }
    #[test]
    fn final_packet_validation_includes_json_structure_and_binary_bytes() {
        let value = json!([{"text":"\u{1}"}]);
        let bytes = serde_json::to_string(&value).unwrap().len();
        assert!(encode_result(&value, &[vec![0; RESULT_BYTES - bytes]]).is_ok());
        assert_eq!(
            encode_result(&value, &[vec![0; RESULT_BYTES - bytes + 1]])
                .unwrap_err()
                .0,
            "SQLITE_RESULT_LIMIT"
        );
    }
}
