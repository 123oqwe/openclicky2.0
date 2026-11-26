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
///
/// The source is a parameter, not a constructor call, so the failure path can
/// be tested without a microphone.
pub fn request_microphone(audio: &dyn openclicky_core::ports::AudioSource) -> bool {
    if audio.start().is_err() {
        return false;
    }
    std::thread::sleep(std::time::Duration::from_millis(400));
    audio.stop().is_ok()
}

/// What asking for a permission can result in. A prompt is asynchronous, so the
/// caller renders this instead of re-probing: a probe taken while the dialog is
/// still on screen reports the state from before it was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionOutcome {
    Prompted,
    OpenedSettings,
    AlreadyGranted,
    Unknown,
}

impl PermissionOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prompted => "prompted",
            Self::OpenedSettings => "opened_settings",
            Self::AlreadyGranted => "already_granted",
            Self::Unknown => "unknown",
        }
    }
}

/// The decision, with no side effects, so it can be checked without opening a
/// settings window on the test machine.
pub fn plan_request(kind: &str, state: PermissionState) -> PermissionOutcome {
    match kind {
        "screen_recording" if state.screen_recording => PermissionOutcome::AlreadyGranted,
        "screen_recording" => PermissionOutcome::Prompted,
        "accessibility" if state.accessibility => PermissionOutcome::AlreadyGranted,
        "accessibility" => PermissionOutcome::OpenedSettings,
        // A refusal cannot be undone by asking again. Only the settings row can.
        "microphone" => match state.microphone {
            MicrophoneAccess::Granted => PermissionOutcome::AlreadyGranted,
            MicrophoneAccess::Denied => PermissionOutcome::OpenedSettings,
            MicrophoneAccess::NotDetermined => PermissionOutcome::Prompted,
        },
        _ => PermissionOutcome::Unknown,
    }
}

pub fn request(kind: &str, audio: &dyn openclicky_core::ports::AudioSource) -> PermissionOutcome {
    let outcome = plan_request(kind, probe());
    match (kind, outcome) {
        ("screen_recording", PermissionOutcome::Prompted) => {
            request_screen_recording();
        }
        ("microphone", PermissionOutcome::Prompted) => {
            request_microphone(audio);
        }
        ("accessibility", PermissionOutcome::OpenedSettings) => {
            open_settings("Privacy_Accessibility")
        }
        ("microphone", PermissionOutcome::OpenedSettings) => open_settings("Privacy_Microphone"),
        _ => {}
    }
    outcome
}

fn open_settings(anchor: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg(format!(
                "x-apple.systempreferences:com.apple.preference.security?{anchor}"
            ))
            .spawn();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = anchor;
    }
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

    struct FakeAudio {
        opens: bool,
    }

    impl openclicky_core::ports::AudioSource for FakeAudio {
        fn start(&self) -> openclicky_core::error::Result<()> {
            if self.opens {
                Ok(())
            } else {
                Err(openclicky_core::error::Error::NoPermission(
                    openclicky_core::error::Permission::Microphone,
                ))
            }
        }
        fn stop(&self) -> openclicky_core::error::Result<openclicky_core::ports::AudioClip> {
            Ok(openclicky_core::ports::AudioClip {
                pcm: Vec::new(),
                sample_rate: 16_000,
                channels: 1,
            })
        }
        fn snapshot(&self) -> openclicky_core::error::Result<openclicky_core::ports::AudioClip> {
            self.stop()
        }
        fn is_recording(&self) -> bool {
            false
        }
    }

    #[test]
    fn requesting_the_microphone_reports_whether_the_stream_opened() {
        assert!(request_microphone(&FakeAudio { opens: true }));
        assert!(
            !request_microphone(&FakeAudio { opens: false }),
            "a refused stream must not be reported as a granted microphone"
        );
    }

    #[test]
    fn each_state_of_the_microphone_leads_to_the_right_action() {
        assert_eq!(
            plan_request("microphone", state(MicrophoneAccess::NotDetermined)),
            PermissionOutcome::Prompted,
            "a first run must raise the system prompt"
        );
        assert_eq!(
            plan_request("microphone", state(MicrophoneAccess::Denied)),
            PermissionOutcome::OpenedSettings,
            "asking again after a refusal shows nothing"
        );
        assert_eq!(
            plan_request("microphone", state(MicrophoneAccess::Granted)),
            PermissionOutcome::AlreadyGranted
        );
    }

    #[test]
    fn an_unknown_permission_name_is_not_treated_as_handled() {
        assert_eq!(
            plan_request("teleportation", state(MicrophoneAccess::Granted)),
            PermissionOutcome::Unknown
        );
        assert_eq!(PermissionOutcome::Unknown.as_str(), "unknown");
    }

    #[test]
    fn permission_outcomes_serialise_to_the_strings_the_window_reads() {
        assert_eq!(PermissionOutcome::Prompted.as_str(), "prompted");
        assert_eq!(
            PermissionOutcome::OpenedSettings.as_str(),
            "opened_settings"
        );
        assert_eq!(
            PermissionOutcome::AlreadyGranted.as_str(),
            "already_granted"
        );
    }

    #[test]
    fn the_three_microphone_states_keep_their_wire_names() {
        assert_eq!(MicrophoneAccess::NotDetermined.as_str(), "not_determined");
        assert_eq!(MicrophoneAccess::Denied.as_str(), "denied");
        assert_eq!(MicrophoneAccess::Granted.as_str(), "granted");
    }

    #[test]
    fn only_granted_counts_as_present() {
        assert!(!missing_microphone(MicrophoneAccess::Granted));
        assert!(missing_microphone(MicrophoneAccess::NotDetermined));
        assert!(missing_microphone(MicrophoneAccess::Denied));
    }
}
