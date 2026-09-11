// Cocoa CEF protocol integration follows cef-rs; see ../LICENSE.cef-rs.
use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use objc2::{define_class, msg_send, rc::Retained, runtime::Bool, DefinedClass, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEvent};
use std::cell::Cell;

#[derive(Default)]
pub struct Ivars {
    sending: Cell<Bool>,
}
define_class!(
    #[unsafe(super(NSApplication))]
    #[ivars = Ivars]
    pub struct BrowserApplication;
    impl BrowserApplication {
        #[unsafe(method(sendEvent:))]
        unsafe fn send_event(&self, event: &NSEvent) {
            let previous = self.ivars().sending.replace(Bool::YES);
            let _: () = msg_send![super(self), sendEvent: event];
            self.ivars().sending.set(previous);
        }
    }
    unsafe impl CrAppControlProtocol for BrowserApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        unsafe fn set_handling(&self, value: Bool) { self.ivars().sending.set(value); }
    }
    unsafe impl CrAppProtocol for BrowserApplication {
        #[unsafe(method(isHandlingSendEvent))]
        unsafe fn handling(&self) -> Bool { self.ivars().sending.get() }
    }
    unsafe impl CefAppProtocol for BrowserApplication {}
);
pub fn initialize() {
    use objc2::ClassType;
    let _: Retained<BrowserApplication> =
        unsafe { msg_send![BrowserApplication::class(), sharedApplication] };
    NSApplication::sharedApplication(MainThreadMarker::new().unwrap())
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}
