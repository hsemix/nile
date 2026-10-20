//! Dragging files out of the window: when a page's drag carries files (`dataTransfer.setFiles`) and the
//! pointer leaves the window, the shell starts the system's own drag with them, so they can be dropped
//! on the desktop, in a file manager or in another app. macOS: an `NSDraggingSession` on the window's
//! view. Windows: `DoDragDrop` with a shell data object (`dragout_win`). Elsewhere: not supported.

/// Start a system drag of `files` from `window`. False when it could not start (or the platform has none).
#[cfg(target_os = "macos")]
pub fn start(window: &winit::window::Window, files: &[String]) -> bool {
    mac::start(window, files)
}

#[cfg(windows)]
pub fn start(window: &winit::window::Window, files: &[String]) -> bool {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(RawWindowHandle::Win32(h)) = window.window_handle().map(|h| h.as_raw()) else { return false };
    crate::dragout_win::drag_files(h.hwnd.get(), files)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn start(_window: &winit::window::Window, _files: &[String]) -> bool {
    false
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::{define_class, rc::Retained, runtime::ProtocolObject, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSEventType, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource, NSPasteboardItem, NSPasteboardWriting, NSView, NSWorkspace};
    use objc2_foundation::{MainThreadMarker, NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and the class has no ivars or Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "NileDragSource"]
        struct DragSource;

        unsafe impl NSObjectProtocol for DragSource {}

        unsafe impl NSDraggingSource for DragSource {
            #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
            fn source_operation_mask(&self, _session: &NSDraggingSession, _context: NSDraggingContext) -> NSDragOperation {
                // Copies only: the page's files stay where they are.
                NSDragOperation::Copy
            }
        }
    );

    thread_local! {
        /// The session keeps only a weak reference to its source.
        static SOURCE: std::cell::RefCell<Option<Retained<DragSource>>> = const { std::cell::RefCell::new(None) };
    }

    pub fn start(window: &winit::window::Window, files: &[String]) -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };
        let Ok(RawWindowHandle::AppKit(handle)) = window.window_handle().map(|h| h.as_raw()) else { return false };
        // SAFETY: winit gives the window's live NSView, and this runs on the main thread.
        let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        let app = NSApplication::sharedApplication(mtm);
        // The drag session starts from the mouse event being handled right now.
        let Some(event) = app.currentEvent() else { return false };
        // AppKit throws for anything but a mouse-down or drag (a mouse-exited event, say), and an
        // exception through Rust frames aborts the program: refuse here, and catch what is left below.
        if !matches!(event.r#type(), NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged | NSEventType::RightMouseDown | NSEventType::RightMouseDragged | NSEventType::OtherMouseDown | NSEventType::OtherMouseDragged) {
            return false;
        }
        let at = view.convertPoint_fromView(event.locationInWindow(), None);
        let workspace = NSWorkspace::sharedWorkspace();
        let mut items: Vec<Retained<NSDraggingItem>> = Vec::new();
        for (i, file) in files.iter().enumerate() {
            // An absolute path, so the file URL doesn't depend on the working directory.
            let absolute = std::path::absolute(file).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| file.clone());
            let path = NSString::from_str(&absolute);
            let url = NSURL::fileURLWithPath(&path);
            // An explicit pasteboard item carrying the file URL: handing the NSURL itself to the item left the
            // drag pasteboard empty ("0 items on the pasteboard").
            let pb_item = NSPasteboardItem::new();
            let url_text = url.absoluteString().unwrap_or_else(|| NSString::from_str(&format!("file://{absolute}")));
            if !pb_item.setString_forType(&url_text, &NSString::from_str("public.file-url")) {
                eprintln!("[drag] could not put {absolute} on the drag pasteboard");
            }
            let writer: &ProtocolObject<dyn NSPasteboardWriting> = ProtocolObject::from_ref(&*pb_item);
            let item = NSDraggingItem::initWithPasteboardWriter(mtm.alloc(), writer);
            let icon = workspace.iconForFile(&path);
            let offset = 6.0 * i as f64;
            let frame = NSRect::new(NSPoint::new(at.x - 24.0 + offset, at.y - 24.0 - offset), NSSize::new(48.0, 48.0));
            // SAFETY: the contents are an NSImage, which a dragging item accepts.
            unsafe { item.setDraggingFrame_contents(frame, Some(&icon)) };
            items.push(item);
        }
        if items.is_empty() {
            return false;
        }
        let source: Retained<DragSource> = unsafe { objc2::msg_send![mtm.alloc::<DragSource>(), init] };
        let array = NSArray::from_retained_slice(&items);
        let began = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            view.beginDraggingSessionWithItems_event_source(&array, &event, ProtocolObject::from_ref(&*source));
        }));
        if let Err(e) = began {
            eprintln!("[drag] AppKit refused the drag: {e:?}");
            return false;
        }
        SOURCE.with(|s| *s.borrow_mut() = Some(source));
        true
    }
}
