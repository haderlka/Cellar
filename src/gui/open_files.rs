//! Files macOS asks Cellar to open: double-clicked in Finder, "Open With",
//! dropped on the Dock icon. macOS sends these as an Apple Event instead of
//! command-line arguments, and winit doesn't forward it, so on macOS we
//! install our own handler. On Windows and Linux such files arrive as
//! arguments (see `main`) and this module does nothing.
//!
//! The handler queues the paths; `GuiApp` takes them every frame. A file
//! that launched the app is queued before the window exists and opened on
//! the first frame.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use eframe::egui;

static PENDING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

/// Start listening for open requests. Call before `eframe::run_native`.
pub fn install() {
    #[cfg(target_os = "macos")]
    macos::install();
}

/// The context to wake when a file arrives while the app is idle.
pub fn set_context(ctx: &egui::Context) {
    let _ = CONTEXT.set(ctx.clone());
}

/// Files requested since the last call, oldest first.
pub fn take() -> Vec<PathBuf> {
    std::mem::take(&mut *PENDING.lock().unwrap_or_else(|e| e.into_inner()))
}

#[cfg(target_os = "macos")]
fn push(paths: impl IntoIterator<Item = PathBuf>) {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).extend(paths);
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;

    use objc2::rc::Retained;
    use objc2::runtime::NSObject;
    use objc2::{define_class, msg_send, sel, ClassType};
    use objc2_foundation::{ns_string, NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter};

    /// Four-character Apple Event codes: 'aevt', 'odoc', '----'.
    const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
    const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
    const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and this class
        // has no Drop impl.
        #[unsafe(super(NSObject))]
        #[name = "CellarOpenDocumentsHandler"]
        struct Handler;

        impl Handler {
            /// AppKit installs its own "open documents" handler while
            /// launching (it would report "cannot open files of this type");
            /// replacing it here, before the launch event is delivered, is
            /// the documented way to take over.
            #[unsafe(method(applicationWillFinishLaunching:))]
            fn will_finish_launching(&self, _notification: &NSNotification) {
                let manager = NSAppleEventManager::sharedAppleEventManager();
                // SAFETY: `self` implements the selector with the signature
                // NSAppleEventManager expects, and lives for the whole run.
                let _: () = unsafe {
                    msg_send![
                        &manager,
                        setEventHandler: self,
                        andSelector: sel!(handleOpenDocuments:withReplyEvent:),
                        forEventClass: CORE_EVENT_CLASS,
                        andEventID: OPEN_DOCUMENTS
                    ]
                };
            }

            #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
            fn handle_open_documents(&self, event: &NSAppleEventDescriptor, _reply: Option<&NSAppleEventDescriptor>) {
                // SAFETY: paramDescriptorForKeyword: takes an AEKeyword
                // (a u32) and returns a nullable descriptor.
                let files: Option<Retained<NSAppleEventDescriptor>> =
                    unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
                let Some(files) = files else { return };
                let path = |d: &NSAppleEventDescriptor| {
                    d.fileURLValue().and_then(|url| url.path()).map(|p| PathBuf::from(p.to_string()))
                };
                // Usually a list of files; a single file may come unwrapped.
                let count = files.numberOfItems();
                if count == 0 {
                    super::push(path(&files));
                } else {
                    super::push((1..=count).filter_map(|i| files.descriptorAtIndex(i)).filter_map(|d| path(&d)));
                }
            }
        }
    );

    pub fn install() {
        // SAFETY: `new` on an NSObject subclass returns an owned instance.
        let handler: Retained<Handler> = unsafe { msg_send![Handler::class(), new] };
        // SAFETY: `handler` implements the selector, taking an NSNotification.
        // NSApplicationWillFinishLaunchingNotification's value is its name.
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &handler,
                sel!(applicationWillFinishLaunching:),
                Some(ns_string!("NSApplicationWillFinishLaunchingNotification")),
                None,
            );
        }
        // The notification center and Apple Event manager don't retain
        // their targets; keep the handler for the rest of the process.
        std::mem::forget(handler);
    }
}
