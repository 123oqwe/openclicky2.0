//! Model-external authorization. Ordering matters: budget and permission are
//! checked before any expensive or irreversible work.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Risk {
    Read,
    Write,
    Destructive,
    External,
}

impl Risk {
    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Read => "read",
            Risk::Write => "write",
            Risk::Destructive => "destructive",
            Risk::External => "external",
        }
    }

    pub fn needs_approval(self) -> bool {
        matches!(self, Risk::Destructive)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Approvals {
    /// Granted for the current conversation (task).
    pub conversation_granted: bool,
    /// User said "not now"; no more asking until this instant.
    pub denied_until: Option<DateTime<Utc>>,
}

impl Approvals {
    pub fn grant(&mut self) {
        self.conversation_granted = true;
        self.denied_until = None;
    }

    /// `Not now` blocks re-asking for 30 minutes (SRC-PRD K4).
    pub fn deny(&mut self, now: DateTime<Utc>) {
        self.denied_until = Some(now + Duration::minutes(30));
    }

    pub fn blocked(&self, now: DateTime<Utc>) -> bool {
        matches!(self.denied_until, Some(t) if now < t)
    }
}

pub struct Policy;

impl Policy {
    pub fn decide(&self, risk: Risk, approvals: &Approvals, now: DateTime<Utc>) -> Approval {
        match risk {
            Risk::Read => Approval::Allow,
            Risk::External => Approval::Allow,
            Risk::Write => Approval::Allow,
            Risk::Destructive => {
                if approvals.conversation_granted {
                    Approval::Allow
                } else if approvals.blocked(now) {
                    Approval::Deny
                } else {
                    Approval::Ask
                }
            }
        }
    }

    /// Writes outside the workspace always need approval, independent of risk.
    pub fn decide_write(
        &self,
        outside_workspace: bool,
        approvals: &Approvals,
        now: DateTime<Utc>,
    ) -> Approval {
        if !outside_workspace {
            return Approval::Allow;
        }
        if approvals.conversation_granted {
            Approval::Allow
        } else if approvals.blocked(now) {
            Approval::Deny
        } else {
            Approval::Ask
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_never_ask() {
        let p = Policy;
        let a = Approvals::default();
        assert_eq!(p.decide(Risk::Read, &a, Utc::now()), Approval::Allow);
    }

    #[test]
    fn destructive_asks_then_respects_not_now() {
        let p = Policy;
        let mut a = Approvals::default();
        let now = Utc::now();
        assert_eq!(p.decide(Risk::Destructive, &a, now), Approval::Ask);
        a.deny(now);
        assert_eq!(
            p.decide(Risk::Destructive, &a, now + Duration::minutes(1)),
            Approval::Deny
        );
        assert_eq!(
            p.decide(Risk::Destructive, &a, now + Duration::minutes(31)),
            Approval::Ask
        );
    }

    #[test]
    fn grants_cover_the_conversation() {
        let p = Policy;
        let mut a = Approvals::default();
        a.grant();
        assert_eq!(p.decide(Risk::Destructive, &a, Utc::now()), Approval::Allow);
    }

    #[test]
    fn write_outside_workspace_asks() {
        let p = Policy;
        let a = Approvals::default();
        assert_eq!(p.decide_write(false, &a, Utc::now()), Approval::Allow);
        assert_eq!(p.decide_write(true, &a, Utc::now()), Approval::Ask);
    }
}
