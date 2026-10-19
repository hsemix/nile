//! One running copy of an app: the second launch hands its arguments to the first and exits.
//!
//! The first instance listens on a private local endpoint named after the app's id: a Unix domain
//! socket in a per-user folder on macOS and Linux, a loopback TCP port (written to a file in the
//! user's local app data) on Windows. A launch that finds the endpoint alive sends one line of JSON
//! (`{"arguments": [...], "cwd": "..."}`) and waits a moment for the answer. A stale endpoint
//! (the owner crashed) is replaced.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::Duration;

/// What `acquire` decided.
pub enum Role {
    /// This is the first instance: keep running. The guard removes the endpoint on drop.
    Primary(Guard),
    /// Another instance has the app and has been given the arguments: exit.
    Secondary,
}

pub struct Guard {
    path: PathBuf,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A short, file-name-safe digest of the app's id.
fn digest(id: &str) -> String {
    // FNV-1a, 64 bit: enough to tell apps apart, and stable between builds.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// A per-user folder for the endpoint, created private.
fn folder() -> Option<PathBuf> {
    #[cfg(unix)]
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| std::env::temp_dir().join(format!("nile-{}", std::env::var("USER").unwrap_or_default())));
    #[cfg(not(unix))]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?.join("nile");
    std::fs::create_dir_all(&base).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700));
    }
    Some(base)
}

/// Whether another instance of app `id` is alive right now (a connection that sends nothing).
pub fn is_running(id: &str) -> bool {
    folder().is_some_and(|dir| imp::is_running(&dir, &digest(id)))
}

/// Take the app `id`, or hand `payload` (one line of JSON) to the instance that has it. `on_message`
/// runs on a background thread for each message a later launch sends.
pub fn acquire(id: &str, payload: &str, on_message: impl Fn(String) + Send + 'static) -> Role {
    let Some(dir) = folder() else { return Role::Primary(Guard { path: PathBuf::new() }) };
    imp::acquire(dir, &digest(id), payload, on_message)
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::os::unix::net::{UnixListener, UnixStream};

    fn send(path: &std::path::Path, payload: &str) -> std::io::Result<()> {
        let mut stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(payload.replace('\n', " ").as_bytes())?;
        stream.write_all(b"\n")?;
        // Wait for the first instance to say it got the message.
        let mut answer = String::new();
        BufReader::new(stream).read_line(&mut answer)?;
        Ok(())
    }

    pub fn is_running(dir: &std::path::Path, name: &str) -> bool {
        UnixStream::connect(dir.join(format!("{name}.sock"))).is_ok()
    }

    pub fn acquire(dir: PathBuf, name: &str, payload: &str, on_message: impl Fn(String) + Send + 'static) -> Role {
        let path = dir.join(format!("{name}.sock"));
        for _ in 0..3 {
            if path.exists() && send(&path, payload).is_ok() {
                return Role::Secondary;
            }
            // Nobody answered: the socket is left over from a crash (or doesn't exist). Take it.
            let _ = std::fs::remove_file(&path);
            match UnixListener::bind(&path) {
                Ok(listener) => {
                    std::thread::spawn(move || {
                        for stream in listener.incoming().flatten() {
                            let mut line = String::new();
                            let mut reader = BufReader::new(&stream);
                            if reader.read_line(&mut line).is_ok() && !line.trim().is_empty() {
                                on_message(line.trim().to_string());
                            }
                            let _ = (&stream).write_all(b"ok\n");
                        }
                    });
                    return Role::Primary(Guard { path });
                }
                // Another launch bound it a moment ago: try talking to it.
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        Role::Primary(Guard { path })
    }
}

#[cfg(not(unix))]
mod imp {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    fn send(port_file: &std::path::Path, payload: &str) -> std::io::Result<()> {
        let port: u16 = std::fs::read_to_string(port_file)?.trim().parse().map_err(|_| std::io::ErrorKind::InvalidData)?;
        let mut stream = TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_secs(1))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(payload.replace('\n', " ").as_bytes())?;
        stream.write_all(b"\n")?;
        let mut answer = String::new();
        BufReader::new(stream).read_line(&mut answer)?;
        Ok(())
    }

    pub fn is_running(dir: &std::path::Path, name: &str) -> bool {
        let Some(port) = std::fs::read_to_string(dir.join(format!("{name}.port"))).ok().and_then(|t| t.trim().parse::<u16>().ok()) else { return false };
        TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(300)).is_ok()
    }

    pub fn acquire(dir: PathBuf, name: &str, payload: &str, on_message: impl Fn(String) + Send + 'static) -> Role {
        let path = dir.join(format!("{name}.port"));
        for _ in 0..3 {
            if path.exists() && send(&path, payload).is_ok() {
                return Role::Secondary;
            }
            let _ = std::fs::remove_file(&path);
            let Ok(listener) = TcpListener::bind(("127.0.0.1", 0)) else { break };
            let Ok(port) = listener.local_addr().map(|a| a.port()) else { break };
            if std::fs::OpenOptions::new().write(true).create_new(true).open(&path).and_then(|mut f| f.write_all(port.to_string().as_bytes())).is_err() {
                // Another launch wrote its port first: talk to that one.
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let mut line = String::new();
                    if BufReader::new(&stream).read_line(&mut line).is_ok() && !line.trim().is_empty() {
                        on_message(line.trim().to_string());
                    }
                    let _ = (&stream).write_all(b"ok\n");
                }
            });
            return Role::Primary(Guard { path });
        }
        Role::Primary(Guard { path })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn a_second_launch_hands_over_its_arguments_and_a_stale_endpoint_is_replaced() {
        let id = format!("nile-test-{}", std::process::id());
        let (tx, rx) = mpsc::channel();
        let first = acquire(&id, "{}", move |m| {
            let _ = tx.send(m);
        });
        let Role::Primary(guard) = first else { panic!("the first launch is the primary") };
        assert!(is_running(&id), "the endpoint answers while the primary lives");
        let second = acquire(&id, r#"{"arguments":["a.txt"],"cwd":"/x"}"#, |_| panic!("the second must not listen"));
        assert!(matches!(second, Role::Secondary));
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), r#"{"arguments":["a.txt"],"cwd":"/x"}"#);
        // The owner goes away (here: its guard drops and removes the endpoint): the next launch is the primary again.
        drop(guard);
        assert!(!is_running(&id));
        let (tx, _rx) = mpsc::channel::<String>();
        let again = acquire(&id, "{}", move |m| {
            let _ = tx.send(m);
        });
        assert!(matches!(again, Role::Primary(_)));
    }
}
