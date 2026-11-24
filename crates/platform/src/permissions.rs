//! Permission probing only; the UI does the guiding.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionState {
    pub screen_recording: bool,
    pub accessibility: bool,
    pub microphone: bool,
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
        microphone: true,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn probe() -> PermissionState {
    PermissionState {
        screen_recording: true,
        accessibility: true,
        microphone: true,
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

pub fn missing_guidance(state: PermissionState) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    if !state.screen_recording {
        out.push("系统设置 → 隐私与安全性 → 屏幕录制 → 勾选 OpenClicky，然后重启应用。");
    }
    if !state.accessibility {
        out.push("系统设置 → 隐私与安全性 → 辅助功能 → 勾选 OpenClicky（听写与工具需要）。");
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
    fn microphone(&self) -> bool {
        probe().microphone
    }
}
