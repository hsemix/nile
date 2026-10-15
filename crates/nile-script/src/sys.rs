//! `@sys` beyond the file system: processes, sockets, open files and the small OS queries
//! (Sciter's `module-sys`, modelled on NodeJS/txiki). Resources live in one process-wide table
//! keyed by id; blocking operations (`read`, `accept`, `wait`...) run on a worker thread that
//! reports back through the host's [`SysState`], and `ScriptHost::tick` hands each result to its
//! promise (`__sysDone` in `web.js`).

use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU32, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};

use base64::Engine as _;

type Done = Arc<Mutex<Vec<(u32, Result<String, String>)>>>;

/// What one script host has in flight.
#[derive(Clone, Default)]
pub(crate) struct SysState {
    done: Done,
    pending: Arc<AtomicUsize>,
}

impl SysState {
    pub fn take_done(&self) -> Vec<(u32, Result<String, String>)> {
        std::mem::take(&mut *self.done.lock().unwrap())
    }

    pub fn pending(&self) -> bool {
        self.pending.load(Ordering::Relaxed) > 0
    }
}

enum Res {
    File(Mutex<std::fs::File>),
    Proc(Mutex<Child>),
    Reader(Mutex<Box<dyn Read + Send>>),
    Writer(Mutex<Option<Box<dyn Write + Send>>>),
    Tcp(Mutex<Tcp>),
    Udp(Mutex<Option<UdpSocket>>),
    /// A named pipe (a Unix domain socket); elsewhere pipes are `Tcp` sockets on the loopback.
    #[cfg(unix)]
    Pipe(Mutex<Pipe>),
    /// A terminal: standard input (0), output (1) or error (2).
    Tty(i32),
}

#[cfg(unix)]
#[derive(Default)]
struct Pipe {
    listener: Option<std::os::unix::net::UnixListener>,
    stream: Option<std::os::unix::net::UnixStream>,
    /// The socket file a listening pipe created, removed when it closes.
    path: Option<std::path::PathBuf>,
}

/// Where a pipe without a socket file (Windows) tells clients which loopback port it listens on.
#[cfg(not(unix))]
fn pipe_port_file(name: &str) -> std::path::PathBuf {
    let clean: String = name.trim_start_matches(r"\\.\pipe\").chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    std::env::temp_dir().join(format!("nile-pipe-{clean}.port"))
}

#[cfg(feature = "tls")]
type TlsStream = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

#[derive(Default)]
struct Tcp {
    /// Set once `tls` has run: reads and writes go through the encrypted session instead.
    #[cfg(feature = "tls")]
    tls: Option<Arc<Mutex<TlsStream>>>,
    stream: Option<TcpStream>,
    listener: Option<TcpListener>,
    bound: Option<SocketAddr>,
}

fn table() -> &'static Mutex<HashMap<u32, Arc<Res>>> {
    static T: OnceLock<Mutex<HashMap<u32, Arc<Res>>>> = OnceLock::new();
    T.get_or_init(Default::default)
}

fn add(r: Res) -> u32 {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    table().lock().unwrap().insert(id, Arc::new(r));
    id
}

fn get(id: &str) -> Result<Arc<Res>, String> {
    id.parse::<u32>().ok().and_then(|i| table().lock().unwrap().get(&i).cloned()).ok_or_else(|| "EBADF: bad resource".to_string())
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn io(e: std::io::Error) -> String {
    let code = match e.kind() {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::AlreadyExists => "EEXIST",
        std::io::ErrorKind::ConnectionRefused => "ECONNREFUSED",
        std::io::ErrorKind::ConnectionReset => "ECONNRESET",
        std::io::ErrorKind::AddrInUse => "EADDRINUSE",
        std::io::ErrorKind::BrokenPipe => "EPIPE",
        std::io::ErrorKind::TimedOut => "ETIMEDOUT",
        _ => "EIO",
    };
    format!("{code}: {e}")
}

/// `{"ip":"127.0.0.1","port":80,"family":4}`.
fn addr_json(a: SocketAddr) -> String {
    format!("{{\"ip\":\"{}\",\"port\":{},\"family\":{}}}", a.ip(), a.port(), if a.is_ipv4() { 4 } else { 6 })
}

/// A `{ip, port}` given as two strings.
fn resolve(ip: &str, port: &str) -> Result<SocketAddr, String> {
    let port: u16 = port.parse().map_err(|_| "EINVAL: bad port".to_string())?;
    let ip = if ip.is_empty() { "0.0.0.0" } else { ip };
    (ip, port).to_socket_addrs().map_err(io)?.next().ok_or_else(|| "EINVAL: bad address".to_string())
}

/// Operations that return at once. Result is the text handed to JS.
pub(crate) fn call(op: &str, a: &[String]) -> Result<String, String> {
    let arg = |i: usize| a.get(i).map(String::as_str).unwrap_or("");
    match op {
        // ---- the OS ----
        "gettimeofday" => {
            let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
            Ok(format!("{{\"sec\":{},\"usec\":{}}}", d.as_secs(), d.subsec_micros()))
        }
        "uname" => {
            let (sysname, release) = if cfg!(target_os = "macos") {
                ("Darwin", unix_release())
            } else if cfg!(windows) {
                ("Windows_NT", "10.0".to_string())
            } else {
                ("Linux", unix_release())
            };
            let host = std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).unwrap_or_default();
            Ok(format!(
                "{{\"sysname\":\"{sysname}\",\"release\":{},\"version\":\"\",\"machine\":\"{}\",\"nodename\":{}}}",
                crate::envmod::json_str(&release),
                std::env::consts::ARCH,
                crate::envmod::json_str(&host)
            ))
        }
        "environ" => Ok(format!("{{{}}}", std::env::vars().map(|(k, v)| format!("{}:{}", crate::envmod::json_str(&k), crate::envmod::json_str(&v))).collect::<Vec<_>>().join(","))),
        "getenv" => Ok(std::env::var(arg(0)).map(|v| format!("S{v}")).unwrap_or_default()),
        "setenv" => {
            if arg(0).is_empty() || arg(0).contains('=') || arg(0).contains('\0') || arg(1).contains('\0') {
                return Err("EINVAL: bad variable name".into());
            }
            std::env::set_var(arg(0), arg(1));
            Ok(String::new())
        }
        "unsetenv" => {
            if arg(0).is_empty() || arg(0).contains('=') || arg(0).contains('\0') {
                return Err("EINVAL: bad variable name".into());
            }
            std::env::remove_var(arg(0));
            Ok(String::new())
        }
        "cwd" => std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).map_err(io),
        "chdir" => std::env::set_current_dir(arg(0)).map(|()| String::new()).map_err(io),
        "homedir" => Ok(std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default()),
        "tmpdir" => Ok(std::env::temp_dir().to_string_lossy().into_owned()),
        "exepath" => std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).map_err(io),
        "isatty" => {
            use std::io::IsTerminal;
            Ok(match arg(0) {
                "0" => std::io::stdin().is_terminal(),
                "1" => std::io::stdout().is_terminal(),
                "2" => std::io::stderr().is_terminal(),
                _ => false,
            }
            .to_string())
        }
        "random" => {
            // Not cryptographic: each `RandomState` is seeded by the OS.
            use std::hash::{BuildHasher, Hasher};
            let n: usize = arg(0).parse().unwrap_or(0).min(1 << 20);
            let mut out = Vec::with_capacity(n + 8);
            while out.len() < n {
                let mut h = std::collections::hash_map::RandomState::new().build_hasher();
                h.write_usize(out.len());
                out.extend_from_slice(&h.finish().to_le_bytes());
            }
            out.truncate(n);
            Ok(b64().encode(out))
        }
        // ---- files ----
        "fopen" => {
            let flags = arg(1);
            let mut o = std::fs::OpenOptions::new();
            let plus = flags.contains('+');
            match flags.chars().next().unwrap_or('r') {
                'w' => o.write(true).create(true).truncate(true).read(plus),
                'a' => o.append(true).create(true).read(plus),
                _ => o.read(true).write(plus),
            };
            if flags.contains('x') {
                o.create_new(true);
            }
            let path = crate::envmod::path_of(arg(0));
            o.open(&path).map(|f| add(Res::File(Mutex::new(f))).to_string()).map_err(io)
        }
        "fread" => {
            let Res::File(f) = &*get(arg(0))? else { return Err("EBADF: not a file".into()) };
            let mut f = f.lock().unwrap();
            if let Ok(p) = arg(2).parse::<u64>() {
                f.seek(SeekFrom::Start(p)).map_err(io)?;
            }
            let len: u64 = arg(1).parse().unwrap_or(u64::MAX);
            let mut buf = Vec::new();
            Read::by_ref(&mut *f).take(len).read_to_end(&mut buf).map_err(io)?;
            Ok(b64().encode(buf))
        }
        "fwrite" => {
            let Res::File(f) = &*get(arg(0))? else { return Err("EBADF: not a file".into()) };
            let mut f = f.lock().unwrap();
            if let Ok(p) = arg(2).parse::<u64>() {
                f.seek(SeekFrom::Start(p)).map_err(io)?;
            }
            let data = b64().decode(arg(1)).map_err(|e| e.to_string())?;
            f.write_all(&data).map_err(io)?;
            Ok(data.len().to_string())
        }
        "fstat" => {
            let Res::File(f) = &*get(arg(0))? else { return Err("EBADF: not a file".into()) };
            let m = f.lock().unwrap().metadata().map_err(io)?;
            Ok(format!("{{\"isFile\":{},\"isDirectory\":{},\"st_size\":{}}}", m.is_file(), m.is_dir(), m.len()))
        }
        // ---- processes ----
        // args: cwd, stdin, stdout, stderr, count of env pairs, pairs..., program, arguments...
        "spawn" => {
            let n: usize = arg(4).parse().unwrap_or(0);
            let rest = a.get(5 + n * 2..).unwrap_or(&[]);
            let Some(program) = rest.first() else { return Err("EINVAL: nothing to run".into()) };
            let mut cmd = Command::new(program);
            cmd.args(&rest[1..]);
            if !arg(0).is_empty() {
                cmd.current_dir(arg(0));
            }
            for pair in a[5..5 + n * 2].chunks(2) {
                cmd.env(&pair[0], &pair[1]);
            }
            let stdio = |mode: &str| match mode {
                "inherit" => Stdio::inherit(),
                "ignore" => Stdio::null(),
                _ => Stdio::piped(),
            };
            cmd.stdin(stdio(arg(1))).stdout(stdio(arg(2))).stderr(stdio(arg(3)));
            let mut child = cmd.spawn().map_err(io)?;
            let pid = child.id();
            let stdin = child.stdin.take().map(|s| add(Res::Writer(Mutex::new(Some(Box::new(s))))));
            let stdout = child.stdout.take().map(|s| add(Res::Reader(Mutex::new(Box::new(s)))));
            let stderr = child.stderr.take().map(|s| add(Res::Reader(Mutex::new(Box::new(s)))));
            let id = add(Res::Proc(Mutex::new(child)));
            let opt = |v: Option<u32>| v.map_or("null".to_string(), |v| v.to_string());
            Ok(format!("{{\"id\":{id},\"pid\":{pid},\"stdin\":{},\"stdout\":{},\"stderr\":{}}}", opt(stdin), opt(stdout), opt(stderr)))
        }
        "kill" => {
            let res = get(arg(0))?;
            let Res::Proc(c) = &*res else { return Err("EBADF: not a process".into()) };
            let killed = c.lock().unwrap().kill();
            killed.map(|()| String::new()).map_err(io)
        }
        // ---- sockets ----
        "tcp_new" => Ok(add(Res::Tcp(Mutex::default())).to_string()),
        "udp_new" => Ok(add(Res::Udp(Mutex::new(None))).to_string()),
        // ---- named pipes ----
        #[cfg(unix)]
        "pipe_new" => Ok(add(Res::Pipe(Mutex::default())).to_string()),
        #[cfg(not(unix))]
        "pipe_new" => Ok(add(Res::Tcp(Mutex::default())).to_string()),
        "pipe_bind" => {
            #[cfg(unix)]
            {
                let Res::Pipe(p) = &*get(arg(0))? else { return Err("EBADF: not a pipe".into()) };
                let path = std::path::PathBuf::from(arg(1));
                // A socket file nobody listens on is left over from a crash: reuse the name.
                if path.exists() && std::os::unix::net::UnixStream::connect(&path).is_err() {
                    let _ = std::fs::remove_file(&path);
                }
                let l = std::os::unix::net::UnixListener::bind(&path).map_err(io)?;
                let mut p = p.lock().unwrap();
                p.listener = Some(l);
                p.path = Some(path);
                Ok(String::new())
            }
            #[cfg(not(unix))]
            {
                let Res::Tcp(t) = &*get(arg(0))? else { return Err("EBADF: not a pipe".into()) };
                let l = TcpListener::bind(("127.0.0.1", 0)).map_err(io)?;
                let file = pipe_port_file(arg(1));
                if file.exists() && l.local_addr().is_ok() {
                    // Another pipe of that name is listening unless its port refuses connections.
                    let live = std::fs::read_to_string(&file).ok().and_then(|p| p.trim().parse::<u16>().ok()).is_some_and(|p| TcpStream::connect(("127.0.0.1", p)).is_ok());
                    if live {
                        return Err("EADDRINUSE: the pipe name is taken".into());
                    }
                }
                std::fs::write(&file, l.local_addr().map_err(io)?.port().to_string()).map_err(io)?;
                let mut t = t.lock().unwrap();
                t.bound = l.local_addr().ok();
                t.listener = Some(l);
                Ok(String::new())
            }
        }
        "pipe_connect" => {
            #[cfg(unix)]
            {
                let Res::Pipe(p) = &*get(arg(0))? else { return Err("EBADF: not a pipe".into()) };
                let s = std::os::unix::net::UnixStream::connect(arg(1)).map_err(io)?;
                p.lock().unwrap().stream = Some(s);
                Ok(String::new())
            }
            #[cfg(not(unix))]
            {
                let Res::Tcp(t) = &*get(arg(0))? else { return Err("EBADF: not a pipe".into()) };
                let port: u16 = std::fs::read_to_string(pipe_port_file(arg(1))).map_err(io)?.trim().parse().map_err(|_| "ECONNREFUSED: bad pipe".to_string())?;
                t.lock().unwrap().stream = Some(TcpStream::connect(("127.0.0.1", port)).map_err(io)?);
                Ok(String::new())
            }
        }
        // ---- terminals ----
        "tty_new" => match arg(0) {
            "0" => Ok(add(Res::Tty(0)).to_string()),
            "1" => Ok(add(Res::Tty(1)).to_string()),
            "2" => Ok(add(Res::Tty(2)).to_string()),
            _ => Err("EBADF: only standard input, output and error can be terminals".into()),
        },
        // `{"width":W,"height":H}`: `stty size` on Unix terminals, else $COLUMNS/$LINES, else 80x24.
        "tty_size" => {
            #[cfg(unix)]
            let stty = Command::new("stty").arg("size").stdin(Stdio::inherit()).output().ok().and_then(|o| {
                let t = String::from_utf8_lossy(&o.stdout).to_string();
                let mut it = t.split_whitespace().filter_map(|n| n.parse::<u32>().ok());
                Some((it.next()?, it.next()?))
            });
            let env = |k: &str, d: u32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
            #[cfg(unix)]
            let (rows, cols) = stty.unwrap_or((env("LINES", 24), env("COLUMNS", 80)));
            #[cfg(not(unix))]
            let (rows, cols) = (env("LINES", 24), env("COLUMNS", 80));
            Ok(format!("{{\"width\":{cols},\"height\":{rows}}}"))
        }
        // 0 normal, 1 raw, 2 raw with output processing (`stty` on Unix; nothing to do elsewhere).
        "tty_mode" => tty_mode(arg(0)),
        "bind" => match &*get(arg(0))? {
            Res::Tcp(t) => {
                let addr = resolve(arg(1), arg(2))?;
                let l = TcpListener::bind(addr).map_err(io)?;
                let mut t = t.lock().unwrap();
                t.bound = l.local_addr().ok();
                t.listener = Some(l);
                Ok(String::new())
            }
            Res::Udp(u) => {
                let s = UdpSocket::bind(resolve(arg(1), arg(2))?).map_err(io)?;
                *u.lock().unwrap() = Some(s);
                Ok(String::new())
            }
            _ => Err("EBADF: not a socket".into()),
        },
        "listen" => match &*get(arg(0))? {
            Res::Tcp(t) if t.lock().unwrap().listener.is_some() => Ok(String::new()),
            #[cfg(unix)]
            Res::Pipe(p) if p.lock().unwrap().listener.is_some() => Ok(String::new()),
            _ => Err("EINVAL: bind the socket first".into()),
        },
        "sockname" => match &*get(arg(0))? {
            Res::Tcp(t) => {
                let t = t.lock().unwrap();
                t.stream.as_ref().and_then(|s| s.local_addr().ok()).or(t.bound).map(addr_json).ok_or_else(|| "ENOTCONN: not bound".to_string())
            }
            Res::Udp(u) => u.lock().unwrap().as_ref().and_then(|s| s.local_addr().ok()).map(addr_json).ok_or_else(|| "ENOTCONN: not bound".to_string()),
            _ => Err("EBADF: not a socket".into()),
        },
        "peername" => match &*get(arg(0))? {
            Res::Tcp(t) => t.lock().unwrap().stream.as_ref().and_then(|s| s.peer_addr().ok()).map(addr_json).ok_or_else(|| "ENOTCONN: not connected".to_string()),
            Res::Udp(u) => u.lock().unwrap().as_ref().and_then(|s| s.peer_addr().ok()).map(addr_json).ok_or_else(|| "ENOTCONN: not connected".to_string()),
            _ => Err("EBADF: not a socket".into()),
        },
        "shutdown" => match &*get(arg(0))? {
            Res::Tcp(t) => t.lock().unwrap().stream.as_ref().map_or(Err("ENOTCONN: not connected".to_string()), |s| s.shutdown(Shutdown::Write).map(|()| String::new()).map_err(io)),
            _ => Err("EBADF: not a socket".into()),
        },
        // Closing drops the table's reference; a worker still blocked on it ends when its call returns
        // (a socket is shut down first so that happens at once).
        "close" => {
            let id: u32 = arg(0).parse().map_err(|_| "EBADF: bad resource".to_string())?;
            if let Some(r) = table().lock().unwrap().remove(&id) {
                match &*r {
                    Res::Tcp(t) => {
                        if let Some(s) = &t.lock().unwrap().stream {
                            let _ = s.shutdown(Shutdown::Both);
                        }
                    }
                    Res::Writer(w) => *w.lock().unwrap() = None,
                    #[cfg(unix)]
                    Res::Pipe(p) => {
                        let p = p.lock().unwrap();
                        if let Some(s) = &p.stream {
                            let _ = s.shutdown(Shutdown::Both);
                        }
                        if let Some(path) = &p.path {
                            let _ = std::fs::remove_file(path);
                        }
                    }
                    _ => {}
                }
            }
            Ok(String::new())
        }
        _ => Err(format!("ENOSYS: unknown operation {op}")),
    }
}

/// 0 normal, 1 raw, 2 raw with output processing (`stty` on Unix; nothing to do elsewhere).
#[cfg(unix)]
fn tty_mode(mode: &str) -> Result<String, String> {
    let args: &[&str] = match mode {
        "1" => &["raw", "-echo"],
        "2" => &["cbreak", "-echo"],
        _ => &["sane"],
    };
    let ok = Command::new("stty").args(args).stdin(Stdio::inherit()).status().is_ok_and(|s| s.success());
    if ok { Ok(String::new()) } else { Err("ENOTTY: not a terminal".into()) }
}

#[cfg(not(unix))]
fn tty_mode(_mode: &str) -> Result<String, String> {
    Ok(String::new())
}

fn unix_release() -> String {
    Command::new("uname").arg("-r").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// Operations that may block: run on a worker thread; the result is queued under `op_id`.
pub(crate) fn start(state: &SysState, op_id: u32, op: String, args: Vec<String>) {
    state.pending.fetch_add(1, Ordering::Relaxed);
    let (done, pending) = (state.done.clone(), state.pending.clone());
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&op, &args))).unwrap_or_else(|_| Err("EIO: the operation panicked".into()));
        done.lock().unwrap().push((op_id, r));
        pending.fetch_sub(1, Ordering::Relaxed);
    });
}

fn run(op: &str, a: &[String]) -> Result<String, String> {
    let arg = |i: usize| a.get(i).map(String::as_str).unwrap_or("");
    let res = get(arg(0))?;
    let len: usize = arg(1).parse().unwrap_or(65536).clamp(1, 1 << 24);
    // Read results: `D` + data, or `E` at the end of the stream.
    let read_from = |r: &mut dyn Read| {
        let mut buf = vec![0u8; len];
        match r.read(&mut buf) {
            Ok(0) => Ok("E".to_string()),
            Ok(n) => Ok(format!("D{}", b64().encode(&buf[..n]))),
            Err(e) => Err(io(e)),
        }
    };
    match (op, &*res) {
        ("read", Res::Reader(r)) => read_from(&mut **r.lock().unwrap()),
        #[cfg(feature = "tls")]
        ("read", Res::Tcp(t)) if t.lock().unwrap().tls.is_some() => {
            let tls = t.lock().unwrap().tls.clone().expect("checked");
            let id = arg(0).to_string();
            loop {
                let result = {
                    let mut buf = vec![0u8; len];
                    tls.lock().unwrap().read(&mut buf).map(|n| (buf, n))
                };
                match result {
                    Ok((_, 0)) => return Ok("E".into()),
                    Ok((buf, n)) => return Ok(format!("D{}", b64().encode(&buf[..n]))),
                    // Nothing yet: let a write in, and stop if the socket was closed meanwhile.
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                        if get(&id).is_err() {
                            return Ok("E".into());
                        }
                        std::thread::sleep(Duration::from_millis(4));
                    }
                    Err(e) => return Err(io(e)),
                }
            }
        }
        #[cfg(feature = "tls")]
        ("write", Res::Tcp(t)) if t.lock().unwrap().tls.is_some() => {
            let data = b64().decode(arg(1)).map_err(|e| e.to_string())?;
            let tls = t.lock().unwrap().tls.clone().expect("checked");
            let mut s = tls.lock().unwrap();
            s.write_all(&data).and_then(|()| s.flush()).map_err(io)?;
            Ok(data.len().to_string())
        }
        #[cfg(feature = "tls")]
        ("tls", Res::Tcp(t)) => {
            let stream = t.lock().unwrap().stream.as_ref().ok_or("ENOTCONN: not connected")?.try_clone().map_err(io)?;
            let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
            let config = rustls::ClientConfig::builder_with_provider(rustls::crypto::ring::default_provider().into())
                .with_safe_default_protocol_versions()
                .map_err(|e| format!("EIO: {e}"))?
                .with_root_certificates(roots)
                .with_no_client_auth();
            let name = rustls::pki_types::ServerName::try_from(arg(1).to_string()).map_err(|e| format!("EINVAL: {e}"))?;
            let conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| format!("EIO: {e}"))?;
            let mut tls = rustls::StreamOwned::new(conn, stream);
            // Finish the handshake now, so a bad certificate is an error of `tls()`.
            while tls.conn.is_handshaking() {
                tls.conn.complete_io(&mut tls.sock).map_err(|e| format!("ECONNRESET: TLS handshake failed: {e}"))?;
            }
            tls.sock.set_read_timeout(Some(Duration::from_millis(30))).map_err(io)?;
            t.lock().unwrap().tls = Some(Arc::new(Mutex::new(tls)));
            Ok(String::new())
        }
        #[cfg(not(feature = "tls"))]
        ("tls", Res::Tcp(_)) => Err("ENOSYS: this build has no TLS".into()),
        ("read", Res::Tcp(t)) => {
            let mut s = t.lock().unwrap().stream.as_ref().ok_or("ENOTCONN: not connected")?.try_clone().map_err(io)?;
            read_from(&mut s)
        }
        ("write", Res::Writer(w)) => {
            let data = b64().decode(arg(1)).map_err(|e| e.to_string())?;
            let mut w = w.lock().unwrap();
            let w = w.as_mut().ok_or("EPIPE: closed")?;
            w.write_all(&data).and_then(|()| w.flush()).map_err(io)?;
            Ok(data.len().to_string())
        }
        #[cfg(unix)]
        ("read", Res::Pipe(p)) => {
            let mut s = p.lock().unwrap().stream.as_ref().ok_or("ENOTCONN: not connected")?.try_clone().map_err(io)?;
            read_from(&mut s)
        }
        #[cfg(unix)]
        ("write", Res::Pipe(p)) => {
            let data = b64().decode(arg(1)).map_err(|e| e.to_string())?;
            let mut s = p.lock().unwrap().stream.as_ref().ok_or("ENOTCONN: not connected")?.try_clone().map_err(io)?;
            s.write_all(&data).map_err(io)?;
            Ok(data.len().to_string())
        }
        #[cfg(unix)]
        ("accept", Res::Pipe(p)) => {
            let l = p.lock().unwrap().listener.as_ref().ok_or("EINVAL: not listening")?.try_clone().map_err(io)?;
            let (s, _) = l.accept().map_err(io)?;
            Ok(add(Res::Pipe(Mutex::new(Pipe { stream: Some(s), ..Pipe::default() }))).to_string())
        }
        ("read", Res::Tty(0)) => read_from(&mut std::io::stdin().lock()),
        ("write", Res::Tty(fd)) => {
            let data = b64().decode(arg(1)).map_err(|e| e.to_string())?;
            if *fd == 2 {
                let mut e = std::io::stderr().lock();
                e.write_all(&data).and_then(|()| e.flush()).map_err(io)?;
            } else {
                let mut o = std::io::stdout().lock();
                o.write_all(&data).and_then(|()| o.flush()).map_err(io)?;
            }
            Ok(data.len().to_string())
        }
        ("write", Res::Tcp(t)) => {
            let data = b64().decode(arg(1)).map_err(|e| e.to_string())?;
            let mut s = t.lock().unwrap().stream.as_ref().ok_or("ENOTCONN: not connected")?.try_clone().map_err(io)?;
            s.write_all(&data).map_err(io)?;
            Ok(data.len().to_string())
        }
        ("connect", Res::Tcp(t)) => {
            let s = TcpStream::connect(resolve(arg(1), arg(2))?).map_err(io)?;
            t.lock().unwrap().stream = Some(s);
            Ok(String::new())
        }
        ("accept", Res::Tcp(t)) => {
            let l = t.lock().unwrap().listener.as_ref().ok_or("EINVAL: not listening")?.try_clone().map_err(io)?;
            let (s, _) = l.accept().map_err(io)?;
            Ok(add(Res::Tcp(Mutex::new(Tcp { stream: Some(s), ..Tcp::default() }))).to_string())
        }
        ("wait", Res::Proc(c)) => loop {
            let status = c.lock().unwrap().try_wait().map_err(io)?;
            if let Some(s) = status {
                #[cfg(unix)]
                let signal = std::os::unix::process::ExitStatusExt::signal(&s).unwrap_or(0);
                #[cfg(not(unix))]
                let signal = 0;
                return Ok(format!("{{\"exit_status\":{},\"term_signal\":{signal}}}", s.code().unwrap_or(0)));
            }
            std::thread::sleep(Duration::from_millis(10));
        },
        ("recv", Res::Udp(u)) => {
            let s = u.lock().unwrap().as_ref().ok_or("ENOTCONN: not bound")?.try_clone().map_err(io)?;
            let mut buf = vec![0u8; len.max(65536)];
            let (n, from) = s.recv_from(&mut buf).map_err(io)?;
            Ok(format!("{{\"data\":\"{}\",\"flags\":0,\"addr\":{}}}", b64().encode(&buf[..n]), addr_json(from)))
        }
        // args: socket, data, ip, port (the last two empty on a connected socket)
        ("send", Res::Udp(u)) => {
            let data = b64().decode(arg(1)).map_err(|e| e.to_string())?;
            let mut g = u.lock().unwrap();
            if g.is_none() {
                *g = Some(UdpSocket::bind("0.0.0.0:0").map_err(io)?);
            }
            let s = g.as_ref().expect("bound above");
            let n = if arg(2).is_empty() { s.send(&data) } else { s.send_to(&data, resolve(arg(2), arg(3))?) };
            n.map(|n| n.to_string()).map_err(io)
        }
        ("connect", Res::Udp(u)) => {
            let mut g = u.lock().unwrap();
            if g.is_none() {
                *g = Some(UdpSocket::bind("0.0.0.0:0").map_err(io)?);
            }
            g.as_ref().expect("bound above").connect(resolve(arg(1), arg(2))?).map(|()| String::new()).map_err(io)
        }
        _ => Err(format!("ENOSYS: {op} doesn't apply to this resource")),
    }
}
