//! Permission probing only; the UI does the guiding.

pub use openclicky_core::ports::MicrophoneAccess;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionState {
    pub screen_recording: bool,
    pub accessibility: bool,
    pub microphone: MicrophoneAccess,
}

#[cfg(target_os = "macos")]
pub fn probe() -> PermissionState {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
    }
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    PermissionState {
        screen_recording: unsafe { CGPreflightScreenCaptureAccess() },
        accessibility: unsafe { AXIsProcessTrusted() },
        microphone: microphone_access(),
    }
}

/// `+[AVCaptureDevice authorizationStatusForMediaType:]` without an Objective-C
/// crate. The three answers are 0 not asked, 1 no (parental), 2 no (user),
/// 3 yes. Anything unexpected is reported as "not asked" so the UI offers the
/// prompt instead of sending the user to a settings row for a decision they
/// never made.
#[cfg(target_os = "macos")]
fn microphone_access() -> MicrophoneAccess {
    use std::ffi::c_void;

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {
        static AVMediaTypeAudio: *const c_void;
    }
    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const std::ffi::c_char) -> *mut c_void;
        fn sel_registerName(name: *const std::ffi::c_char) -> *mut c_void;
        #[link_name = "objc_msgSend"]
        fn msg_send_i64(receiver: *mut c_void, sel: *mut c_void, arg: *const c_void) -> i64;
    }

    // SAFETY: both selectors are class methods on classes that AVFoundation
    // registers before main runs, and the integer return matches the declared
    // Objective-C signature (NSInteger).
    let status = unsafe {
        let class = objc_getClass(c"AVCaptureDevice".as_ptr());
        if class.is_null() {
            return MicrophoneAccess::NotDetermined;
        }
        let sel = sel_registerName(c"authorizationStatusForMediaType:".as_ptr());
        if sel.is_null() {
            return MicrophoneAccess::NotDetermined;
        }
        msg_send_i64(class, sel, AVMediaTypeAudio)
    };

    match status {
        3 => MicrophoneAccess::Granted,
        1 | 2 => MicrophoneAccess::Denied,
        _ => MicrophoneAccess::NotDetermined,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn probe() -> PermissionState {
    PermissionState {
        screen_recording: true,
        accessibility: true,
        microphone: MicrophoneAccess::Granted,
    }
}

pub fn request_screen_recording() -> bool {
    #[cfg(target_os = "macos")]
    {
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGRequestScreenCaptureAccess() -> bool;
        }
        unsafe { CGRequestScreenCaptureAccess() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Opens a real input stream for a moment. macOS shows its consent prompt the
/// first time a process touches the microphone, so this is what turns
/// "not determined" into a decision. The prompt is the side effect; the return
/// value only says whether the stream opened.
pub fn request_microphone() -> bool {
    use openclicky_core::ports::AudioSource;
    let audio = crate::audio::CpalAudio::new();
    if audio.start().is_err() {
        return false;
    }
    std::thread::sleep(std::time::Duration::from_millis(400));
    audio.stop().is_ok()
}

pub fn missing_microphone(state: MicrophoneAccess) -> bool {
    !state.grants_input()
}

pub fn missing_guidance(state: PermissionState) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    if !state.screen_recording {
        out.push("系统设置 → 隐私与安全性 → 屏幕录制 → 勾选 OpenClicky，然后重启应用。");
    }
    if !state.accessibility {
        out.push("系统设置 → 隐私与安全性 → 辅助功能 → 勾选 OpenClicky（听写与工具需要）。");
    }
    match state.microphone {
        MicrophoneAccess::NotDetermined => out.push(
            "麦克风还没申请过。按一次热键（或点上面这一行），系统会弹出第一次授权框，允许即可。",
        ),
        MicrophoneAccess::Denied => out.push(
            "麦克风已被拒绝。系统设置 → 隐私与安全性 → 麦克风 → 勾选 OpenClicky，然后重启应用。",
        ),
        MicrophoneAccess::Granted => {}
    }
    out
}

/// The core port implementation: a live snapshot probe.
pub struct SystemProbe;

impl openclicky_core::ports::PermissionProbe for SystemProbe {
    fn screen_recording(&self) -> bool {
        probe().screen_recording
    }
    fn accessibility(&self) -> bool {
        probe().accessibility
    }
    fn microphone(&self) -> MicrophoneAccess {
        probe().microphone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(microphone: MicrophoneAccess) -> PermissionState {
        PermissionState {
            screen_recording: true,
            accessibility: true,
            microphone,
        }
    }

    #[test]
    fn a_microphone_never_asked_is_not_the_same_advice_as_one_refused() {
        let fresh = missing_guidance(state(MicrophoneAccess::NotDetermined));
        let refused = missing_guidance(state(MicrophoneAccess::Denied));
        assert_ne!(fresh, refused, "two different states, two different fixes");
        assert!(
            fresh.iter().any(|g| g.contains("第一次")),
            "a fresh install must be told the prompt is coming: {fresh:?}"
        );
        assert!(
            refused.iter().any(|g| g.contains("麦克风")),
            "a refusal must point at the microphone row: {refused:?}"
        );
    }

    #[test]
    fn granted_microphone_says_nothing() {
        assert!(missing_guidance(state(MicrophoneAccess::Granted)).is_empty());
    }

    #[test]
    fn only_granted_counts_as_present() {
        assert!(!missing_microphone(MicrophoneAccess::Granted));
        assert!(missing_microphone(MicrophoneAccess::NotDetermined));
        assert!(missing_microphone(MicrophoneAccess::Denied));
    }
}
