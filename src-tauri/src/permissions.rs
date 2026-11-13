//! Microphone and System Audio Recording permissions.
//!
//! The microphone goes through minutes-core. Core has no API for System Audio Recording
//! (the TCC service behind Core Audio process taps), so we use the TCC SPI the way Apple's
//! AudioCap sample does. It is loaded with dlopen, so a missing symbol means "unknown", not
//! a crash.
//!
//! Windows (D61) has one microphone switch for the PC, one for the account and one for
//! desktop apps, in Settings, Privacy and security, Microphone. Margin reads them from the
//! registry (win.rs). Windows shows no prompt to a desktop app, so there is nothing to
//! request. Windows has no System Audio permission.

#[cfg(target_os = "macos")]
use minutes_core::macos_permissions::{self, MacPermissionStatus, MicrophoneRequestOutcome};
use serde::Serialize;
#[cfg(target_os = "macos")]
use std::ffi::{c_char, c_int, c_void, CString};
#[cfg(target_os = "macos")]
use std::time::Duration;

#[derive(Debug, Serialize)]
pub struct PermissionState {
    /// "granted" | "denied" | "not-determined" | "unknown"
    pub microphone: &'static str,
    pub system_audio: &'static str,
}

/// Windows: the microphone switches from the registry. "unknown" when Windows has no
/// value there. The System Audio permission does not exist, so it is "unknown" too, and
/// the UI does not show it.
#[cfg(not(target_os = "macos"))]
pub fn state() -> PermissionState {
    PermissionState { microphone: microphone(), system_audio: "unknown" }
}

#[cfg(windows)]
fn microphone() -> &'static str {
    match crate::win::microphone_consent() {
        crate::win::MicConsent::Allowed => "granted",
        crate::win::MicConsent::Denied => "denied",
        crate::win::MicConsent::Unknown => "unknown",
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn microphone() -> &'static str {
    "unknown"
}

/// Windows shows no prompt, so a request only reads the switches again.
#[cfg(not(target_os = "macos"))]
pub fn request_microphone() -> &'static str {
    microphone()
}

#[cfg(not(target_os = "macos"))]
pub fn request_system_audio() -> &'static str {
    "unknown"
}

/// Windows: the Microphone page of Settings. It is the only pane.
#[cfg(windows)]
pub fn open_settings(_pane: &str) {
    if let Err(error) = crate::win::shell_open("ms-settings:privacy-microphone") {
        tracing::warn!(%error, "could not open the microphone settings");
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn open_settings(_pane: &str) {}

#[cfg(target_os = "macos")]
pub fn state() -> PermissionState {
    PermissionState {
        microphone: match macos_permissions::microphone_status() {
            MacPermissionStatus::Granted | MacPermissionStatus::NotNeeded => "granted",
            MacPermissionStatus::Denied | MacPermissionStatus::StaleOrRestartNeeded => "denied",
            MacPermissionStatus::NotDetermined => "not-determined",
            _ => "unknown",
        },
        system_audio: match tcc::preflight("kTCCServiceAudioCapture") {
            Some(0) => "granted",
            Some(1) => "denied",
            Some(_) => "not-determined",
            None => "unknown",
        },
    }
}

#[cfg(target_os = "macos")]
pub fn request_microphone() -> &'static str {
    match macos_permissions::request_microphone_access(&|| false, Duration::from_secs(60)) {
        MicrophoneRequestOutcome::Granted => "granted",
        MicrophoneRequestOutcome::Denied => "denied",
        _ => "not-determined",
    }
}

/// Shows the macOS prompt when undecided. Returns the state afterwards.
#[cfg(target_os = "macos")]
pub fn request_system_audio() -> &'static str {
    match tcc::request("kTCCServiceAudioCapture", Duration::from_secs(60)) {
        Some(true) => "granted",
        Some(false) => "denied",
        None => "unknown",
    }
}

#[cfg(target_os = "macos")]
pub fn open_settings(pane: &str) {
    let anchor = match pane {
        "microphone" => "Privacy_Microphone",
        _ => "Privacy_ScreenCapture",
    };
    let _ = std::process::Command::new("/usr/bin/open")
        .arg(format!("x-apple.systempreferences:com.apple.preference.security?{anchor}"))
        .spawn();
}

#[cfg(target_os = "macos")]
mod tcc {
    use super::*;
    use block2::RcBlock;
    use std::sync::mpsc;

    type CFStringRef = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(alloc: *const c_void, s: *const c_char, encoding: u32) -> CFStringRef;
        fn CFRelease(cf: *const c_void);
    }
    const UTF8: u32 = 0x0800_0100;

    fn symbol(name: &str) -> Option<*mut c_void> {
        unsafe {
            let path = CString::new("/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC").ok()?;
            let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW);
            if handle.is_null() {
                return None;
            }
            let sym = CString::new(name).ok()?;
            let ptr = libc::dlsym(handle, sym.as_ptr());
            (!ptr.is_null()).then_some(ptr)
        }
    }

    fn service(name: &str) -> Option<CFStringRef> {
        let c = CString::new(name).ok()?;
        let s = unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) };
        (!s.is_null()).then_some(s)
    }

    /// 0 = granted, 1 = denied, 2 = not determined.
    pub fn preflight(name: &str) -> Option<c_int> {
        let f: extern "C" fn(CFStringRef, *const c_void) -> c_int =
            unsafe { std::mem::transmute(symbol("TCCAccessPreflight")?) };
        let s = service(name)?;
        let result = f(s, std::ptr::null());
        unsafe { CFRelease(s) };
        Some(result)
    }

    pub fn request(name: &str, timeout: Duration) -> Option<bool> {
        let f: extern "C" fn(CFStringRef, *const c_void, *const c_void) =
            unsafe { std::mem::transmute(symbol("TCCAccessRequest")?) };
        let s = service(name)?;
        let (tx, rx) = mpsc::channel::<bool>();
        let block = RcBlock::new(move |granted: u8| {
            let _ = tx.send(granted != 0);
        });
        f(s, std::ptr::null(), RcBlock::as_ptr(&block) as *const c_void);
        let answer = rx.recv_timeout(timeout).ok();
        unsafe { CFRelease(s) };
        answer
    }
}
