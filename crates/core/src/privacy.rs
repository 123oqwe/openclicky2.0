//! The single gate through which screen capture may happen.
//!
//! Invariant INV-D1: while no gate is open, `ScreenSource::capture_all` must not
//! be called. Tests assert the counter stays at zero.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureReason {
    Hotkey,
    /// A typed question may point too (SRC-PRD C4); it still needs the gate.
    TypedQuestion,
    OnboardingDemo,
    UserRetry,
    Calibration,
}

impl CaptureReason {
    pub fn as_str(self) -> &'static str {
        match self {
            CaptureReason::Hotkey => "hotkey",
            CaptureReason::TypedQuestion => "typed",
            CaptureReason::OnboardingDemo => "onboarding",
            CaptureReason::UserRetry => "retry",
            CaptureReason::Calibration => "calibration",
        }
    }
}

#[derive(Debug, Default)]
pub struct CaptureGate {
    opens: AtomicU64,
    last_reason: AtomicU64,
}

pub struct CaptureGuard<'a> {
    gate: &'a CaptureGate,
    pub reason: CaptureReason,
}

impl CaptureGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Authorise one capture. Only user-initiated actions may construct a reason.
    /// The returned guard is the capability: `ScreenSource::capture_all` requires it.
    pub fn authorize(&self, reason: CaptureReason) -> CaptureGuard<'_> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        self.last_reason.store(reason as u64, Ordering::SeqCst);
        CaptureGuard { gate: self, reason }
    }

    /// Number of times capture was authorised (audit signal).
    pub fn opens(&self) -> u64 {
        self.opens.load(Ordering::SeqCst)
    }

    pub fn last_reason(&self) -> Option<CaptureReason> {
        match self.last_reason.load(Ordering::SeqCst) {
            0 => Some(CaptureReason::Hotkey),
            1 => Some(CaptureReason::OnboardingDemo),
            2 => Some(CaptureReason::UserRetry),
            3 => Some(CaptureReason::Calibration),
            _ => None,
        }
    }
}

impl<'a> CaptureGuard<'a> {
    /// Closes the gate; afterwards capture must not be attempted.
    pub fn close(self) -> u64 {
        self.gate.opens()
    }

    /// Why the gate was opened (audit / tests).
    pub fn reason(&self) -> CaptureReason {
        self.reason
    }
}
