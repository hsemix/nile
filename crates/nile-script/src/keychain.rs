//! `@nile/keychain`: secrets in the operating system's credential store (macOS Keychain, Windows Credential Manager, the
//! Secret Service on Linux) through the `keyring` crate. One entry point, `call(op, args)`; calls can block (an unlock
//! prompt, D-Bus), so `sys::start` runs them on a worker thread when the op is prefixed `keychain:`.
//!
//! Answers: `set` and `remove` -> "" / "true" / "false"; `get` -> "N" (nothing stored) or "V" + text, "B" + base64 for
//! bytes; `status` -> "ok" or the reason the store is unusable. Errors are short messages.

#[cfg(feature = "keychain")]
mod store {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use keyring::{Entry, Error};

    fn entry(service: &str, account: &str) -> Result<Entry, String> {
        if service.is_empty() || account.is_empty() {
            return Err("a service and an account name are required".into());
        }
        Entry::new(service, account).map_err(message)
    }

    fn message(e: Error) -> String {
        match e {
            Error::NoDefaultStore => "the system credential store is not available (is a keyring service running?)".into(),
            Error::NoStorageAccess(inner) => format!("access to the credential store was denied: {inner}"),
            other => other.to_string(),
        }
    }

    pub fn call(op: &str, a: &[String]) -> Result<String, String> {
        let arg = |i: usize| a.get(i).map_or("", String::as_str);
        match op {
            "status" => Ok(match Entry::store_status() {
                Ok(()) => "ok".to_string(),
                Err(e) => message(match e {
                    Error::NoDefaultStore => Error::NoDefaultStore,
                    other => Error::Invalid("store".into(), other.to_string()),
                }),
            }),
            // service, account, kind ("text" | "bytes"), data
            "set" => {
                let e = entry(arg(0), arg(1))?;
                if arg(2) == "bytes" {
                    e.set_secret(&B64.decode(arg(3)).map_err(|_| "invalid data".to_string())?).map_err(message)?;
                } else {
                    e.set_password(arg(3)).map_err(message)?;
                }
                Ok(String::new())
            }
            // service, account, kind
            "get" => {
                let e = entry(arg(0), arg(1))?;
                if arg(2) == "bytes" {
                    match e.get_secret() {
                        Ok(b) => Ok(format!("B{}", B64.encode(b))),
                        Err(Error::NoEntry) => Ok("N".into()),
                        Err(other) => Err(message(other)),
                    }
                } else {
                    match e.get_password() {
                        Ok(s) => Ok(format!("V{s}")),
                        Err(Error::NoEntry) => Ok("N".into()),
                        Err(other) => Err(message(other)),
                    }
                }
            }
            "remove" => match entry(arg(0), arg(1))?.delete_credential() {
                Ok(()) => Ok("true".into()),
                Err(Error::NoEntry) => Ok("false".into()),
                Err(other) => Err(message(other)),
            },
            other => Err(format!("unknown keychain operation: {other}")),
        }
    }
}

pub fn call(op: &str, args: &[String]) -> Result<String, String> {
    #[cfg(feature = "keychain")]
    {
        store::call(op, args)
    }
    #[cfg(not(feature = "keychain"))]
    {
        let _ = args;
        if op == "status" {
            Ok("this build has no credential store".into())
        } else {
            Err("this build has no credential store".into())
        }
    }
}

#[cfg(all(test, feature = "keychain"))]
mod tests {
    use super::*;

    fn run(op: &str, args: &[&str]) -> Result<String, String> {
        call(op, &args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn names_are_required() {
        assert!(run("set", &["", "acct", "text", "x"]).is_err());
        assert!(run("get", &["svc", "", "text"]).is_err());
        assert!(run("nope", &[]).is_err());
    }

    /// Touches the real store of the machine it runs on (a Keychain item, a Credential Manager entry, a Secret Service
    /// item): run by hand with `cargo test -p nile-script keychain -- --ignored`.
    #[test]
    #[ignore]
    fn round_trips_through_the_system_store() {
        let svc = format!("nile-test-{}", std::process::id());
        assert_eq!(run("status", &[]).unwrap(), "ok");
        assert_eq!(run("get", &[&svc, "me", "text"]).unwrap(), "N");
        run("set", &[&svc, "me", "text", "pässword ✓"]).unwrap();
        assert_eq!(run("get", &[&svc, "me", "text"]).unwrap(), "Vpässword ✓");
        run("set", &[&svc, "me", "text", "second"]).unwrap();
        assert_eq!(run("get", &[&svc, "me", "text"]).unwrap(), "Vsecond");
        run("set", &[&svc, "bin", "bytes", "AAEC/w=="]).unwrap();
        assert_eq!(run("get", &[&svc, "bin", "bytes"]).unwrap(), "BAAEC/w==");
        assert_eq!(run("remove", &[&svc, "me"]).unwrap(), "true");
        assert_eq!(run("remove", &[&svc, "me"]).unwrap(), "false");
        assert_eq!(run("get", &[&svc, "me", "text"]).unwrap(), "N");
        run("remove", &[&svc, "bin"]).unwrap();
    }
}
