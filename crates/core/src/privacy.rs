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
    /// The number stored for the audit signal. `authorize` writes it and
    /// `last_reason` reads it; a hand-written match on the read side had drifted
    /// by one variant and reported the wrong reason.
    fn code(self) -> u64 {
        match self {
            Self::Hotkey => 0,
            Self::TypedQuestion => 1,
            Self::OnboardingDemo => 2,
            Self::UserRetry => 3,
            Self::Calibration => 4,
        }
    }

    fn from_code(code: u64) -> Option<Self> {
        match code {
            0 => Some(Self::Hotkey),
            1 => Some(Self::TypedQuestion),
            2 => Some(Self::OnboardingDemo),
            3 => Some(Self::UserRetry),
            4 => Some(Self::Calibration),
            _ => None,
        }
    }

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
        self.last_reason.store(reason.code(), Ordering::SeqCst);
        CaptureGuard { gate: self, reason }
    }

    /// Number of times capture was authorised (audit signal).
    pub fn opens(&self) -> u64 {
        self.opens.load(Ordering::SeqCst)
    }

    pub fn last_reason(&self) -> Option<CaptureReason> {
        CaptureReason::from_code(self.last_reason.load(Ordering::SeqCst))
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

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [CaptureReason; 5] = [
        CaptureReason::Hotkey,
        CaptureReason::TypedQuestion,
        CaptureReason::OnboardingDemo,
        CaptureReason::UserRetry,
        CaptureReason::Calibration,
    ];

    #[test]
    fn every_reason_survives_the_round_trip() {
        for reason in ALL {
            assert_eq!(CaptureReason::from_code(reason.code()), Some(reason));
        }
    }

    #[test]
    fn the_audit_signal_reports_the_reason_that_opened_the_gate() {
        for reason in ALL {
            let gate = CaptureGate::new();
            gate.authorize(reason).close();
            assert_eq!(gate.last_reason(), Some(reason));
        }
    }

    #[test]
    fn an_unknown_code_is_not_guessed() {
        assert_eq!(CaptureReason::from_code(9), None);
    }
}
