//! Non-database extension: bounded background notifications with unsubscribe and cleanup.
use nile_extension_sdk::*;
use std::collections::HashMap;
use std::sync::{
    mpsc::{channel, Sender},
    Mutex,
};
use std::thread::JoinHandle;
use std::time::Duration;
struct Subscription {
    stop: Sender<()>,
    worker: JoinHandle<()>,
}
struct State {
    client: Client,
    subscriptions: Mutex<HashMap<u64, Subscription>>,
}
unsafe extern "C" fn dispatch(user: u64, request: *const NileRequest) -> NileStatus {
    guard(|| {
        let state = &*(user as *const State);
        let request = &*request;
        if request.flags != NILE_SUBSCRIBE {
            return NILE_INVALID;
        }
        let client = state.client;
        let (context, call) = (request.context, request.call);
        let (stop, receiver) = channel();
        let worker = match std::thread::Builder::new()
            .name("nile-notifications".into())
            .spawn(move || {
                for sequence in 1u64.. {
                    if !matches!(
                        receiver.recv_timeout(Duration::from_millis(10)),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    ) {
                        break;
                    }
                    let json = format!("{{\"sequence\":{sequence}}}");
                    let status = client.reply(context, call, Ok(&json), &[], true);
                    if status != NILE_OK && status != NILE_OVERLOADED {
                        break;
                    }
                }
            }) {
            Ok(worker) => worker,
            Err(_) => return NILE_FAILED,
        };
        state
            .subscriptions
            .lock()
            .unwrap()
            .insert(call, Subscription { stop, worker });
        client.reply(context, call, Ok("{\"subscribed\":true}"), &[], false);
        NILE_OK
    })
}
unsafe extern "C" fn cancel(user: u64, _: u64, call: u64) {
    guard(|| {
        let state = &*(user as *const State);
        let subscription = state.subscriptions.lock().unwrap().remove(&call);
        if let Some(subscription) = subscription {
            let _ = subscription.stop.send(());
            let _ = subscription.worker.join();
        }
        NILE_OK
    });
}
unsafe extern "C" fn dispose(user: u64, context: u64) -> NileStatus {
    guard(|| {
        let state = &*(user as *const State);
        let calls: Vec<_> = state
            .subscriptions
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect();
        for call in calls {
            cancel(user, context, call);
        }
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
        if client.0.capabilities & NILE_CAP_EVENTS == 0 {
            return NILE_INVALID;
        }
        let state = Box::new(State {
            client,
            subscriptions: Mutex::default(),
        });
        let user = Box::into_raw(state) as u64;
        *out = NileExtension {
            user,
            required_capabilities: NILE_CAP_EVENTS,
            cancel: Some(cancel),
            context_dispose: Some(dispose),
            shutdown: Some(shutdown),
            ..NileExtension::default()
        };
        (client.0.register_operation.unwrap())(
            client.0.host,
            NileSlice::new(b"notifications.watch"),
            dispatch,
            user,
        )
    })
}
