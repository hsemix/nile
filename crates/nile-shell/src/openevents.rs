//! macOS: the system opening a URL (`myapp://...`) or documents with the running app arrives as Apple events,
//! not as arguments. They become `UserEvent::Application` (`open-url`, `open-file`) for the page's scripts.

use std::sync::OnceLock;

use objc2::{define_class, msg_send, rc::Retained, runtime::{AnyClass, AnyObject}, sel, MainThreadOnly};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString};
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;

static PROXY: OnceLock<EventLoopProxy<UserEvent>> = OnceLock::new();

/// The four-character codes the events use.
const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}
const KEY_DIRECT_OBJECT: u32 = fourcc(b"----");
const CLASS_INTERNET: u32 = fourcc(b"GURL");
const ID_GET_URL: u32 = fourcc(b"GURL");
const CLASS_CORE: u32 = fourcc(b"aevt");
const ID_OPEN_DOCUMENTS: u32 = fourcc(b"odoc");

/// `file:///a%20b` as `/a b`.
fn path_of(url: &str) -> String {
    let rest = url.strip_prefix("file://").unwrap_or(url);
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let mut out = Vec::new();
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(v) = rest.get(i + 1..i + 3).and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn string_value(descriptor: &AnyObject) -> Option<String> {
    let s: Option<Retained<NSString>> = unsafe { msg_send![descriptor, stringValue] };
    s.map(|s| s.to_string())
}

fn send(kind: &str, key: &str, value: String) {
    if let Some(proxy) = PROXY.get() {
        let _ = proxy.send_event(UserEvent::Application(serde_json::json!({ "type": kind, key: value }).to_string()));
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and the class has no ivars or Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NileOpenTarget"]
    struct OpenTarget;

    unsafe impl NSObjectProtocol for OpenTarget {}

    impl OpenTarget {
        #[unsafe(method(handleURL:withReply:))]
        fn handle_url(&self, event: &AnyObject, _reply: &AnyObject) {
            let direct: Option<Retained<AnyObject>> = unsafe { msg_send![event, paramDescriptorForKeyword: KEY_DIRECT_OBJECT] };
            if let Some(url) = direct.as_deref().and_then(string_value) {
                send("open-url", "url", url);
            }
        }

        #[unsafe(method(handleOpen:withReply:))]
        fn handle_open(&self, event: &AnyObject, _reply: &AnyObject) {
            let direct: Option<Retained<AnyObject>> = unsafe { msg_send![event, paramDescriptorForKeyword: KEY_DIRECT_OBJECT] };
            let Some(list) = direct else { return };
            // A list of file URLs (a single file arrives as a list of one on current systems).
            let count: isize = unsafe { msg_send![&*list, numberOfItems] };
            if count == 0 {
                if let Some(url) = string_value(&list) {
                    send("open-file", "path", path_of(&url));
                }
            }
            for i in 1..=count {
                let item: Option<Retained<AnyObject>> = unsafe { msg_send![&*list, descriptorAtIndex: i] };
                if let Some(url) = item.as_deref().and_then(string_value) {
                    send("open-file", "path", path_of(&url));
                }
            }
        }
    }
);

/// Listen for the open-URL and open-documents events. Call on the main thread before the event loop runs,
/// and again once the app has finished launching (AppKit installs its own open-documents handler then).
pub fn install(proxy: EventLoopProxy<UserEvent>) {
    let _ = PROXY.set(proxy);
    add_delegate_method();
    install_handlers();
}

/// `application:openURLs:` on the app's delegate: what AppKit calls for documents opened at launch (before the
/// Apple event handlers could be taken back from it). winit's delegate has no such method, so it is added.
unsafe extern "C-unwind" fn open_urls(_this: *mut AnyObject, _sel: objc2::runtime::Sel, _app: *mut AnyObject, urls: *mut AnyObject) {
    // SAFETY: AppKit passes an NSArray of NSURL.
    let Some(urls) = (unsafe { urls.as_ref() }) else { return };
    let count: usize = unsafe { msg_send![urls, count] };
    for i in 0..count {
        let url: Option<Retained<AnyObject>> = unsafe { msg_send![urls, objectAtIndex: i] };
        let Some(url) = url else { continue };
        let is_file: bool = unsafe { msg_send![&*url, isFileURL] };
        if is_file {
            let path: Option<Retained<NSString>> = unsafe { msg_send![&*url, path] };
            if let Some(path) = path {
                send("open-file", "path", path.to_string());
            }
        } else {
            let text: Option<Retained<NSString>> = unsafe { msg_send![&*url, absoluteString] };
            if let Some(text) = text {
                send("open-url", "url", text.to_string());
            }
        }
    }
}

/// Teach the application delegate `application:openURLs:` (once).
fn add_delegate_method() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    let Some(delegate) = app.delegate() else { return };
    let delegate: &AnyObject = delegate.as_ref();
    let class = delegate.class();
    if class.responds_to(sel!(application:openURLs:)) {
        return;
    }
    // SAFETY: the signature matches `v@:@@`: no result, self, selector, the application and the URL array.
    unsafe {
        let imp: objc2::runtime::Imp = std::mem::transmute::<
            unsafe extern "C-unwind" fn(*mut AnyObject, objc2::runtime::Sel, *mut AnyObject, *mut AnyObject),
            unsafe extern "C-unwind" fn(),
        >(open_urls);
        objc2::ffi::class_addMethod(class as *const AnyClass as *mut AnyClass, sel!(application:openURLs:), imp, c"v@:@@".as_ptr());
    }
}

pub fn install_handlers() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(manager_class) = AnyClass::get(c"NSAppleEventManager") else { return };
    let target: Retained<OpenTarget> = unsafe { msg_send![mtm.alloc::<OpenTarget>(), init] };
    let manager: Retained<AnyObject> = unsafe { msg_send![manager_class, sharedAppleEventManager] };
    unsafe {
        let _: () = msg_send![&*manager, setEventHandler: &*target, andSelector: sel!(handleURL:withReply:), forEventClass: CLASS_INTERNET, andEventID: ID_GET_URL];
        let _: () = msg_send![&*manager, setEventHandler: &*target, andSelector: sel!(handleOpen:withReply:), forEventClass: CLASS_CORE, andEventID: ID_OPEN_DOCUMENTS];
    }
    // The manager keeps no strong reference to its handlers: the target lives as long as the program.
    std::mem::forget(target);
}

#[cfg(test)]
mod tests {
    use super::path_of;

    #[test]
    fn file_urls_become_paths() {
        assert_eq!(path_of("file:///Users/a/My%20Song.mp3"), "/Users/a/My Song.mp3");
        assert_eq!(path_of("file://localhost/tmp/x"), "/tmp/x");
        assert_eq!(path_of("/plain/path"), "/plain/path");
        assert_eq!(path_of("file:///100%"), "/100%");
    }
}
