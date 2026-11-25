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

/// Everything the policy needs to know about one call. Kept explicit so the
/// matrix is testable without a filesystem or a model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CallContext {
    pub risk: Risk,
    /// The resolved path escapes the workspace (only meaningful for Write).
    pub outside_workspace: bool,
    /// The destination host is on the user's allowlist (only meaningful for
    /// External). An empty allowlist means nothing is trusted.
    pub host_allowed: bool,
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

    /// Backwards-compatible name used by older call sites.
    pub fn is_write(self) -> bool {
        matches!(self, Risk::Write)
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
    /// The single authorization matrix (tech-prd SEC-4):
    ///
    /// | risk        | condition          | decision |
    /// |-------------|--------------------|----------|
    /// | Read        | —                  | Allow    |
    /// | Write       | inside workspace   | Allow    |
    /// | Write       | outside workspace  | Ask      |
    /// | External    | host on allowlist  | Allow    |
    /// | External    | any other host     | Ask      |
    /// | Destructive | —                  | Ask      |
    ///
    /// External defaults to Ask because the URL itself is an outbound channel:
    /// there is no honest way to tell a "fetch" from an "upload" by shape, so
    /// only the user's own allowlist may skip the prompt.
    ///
    /// `Deny` only applies after the user chose "Not now" (30 minute window).
    pub fn decide(&self, call: CallContext, approvals: &Approvals, now: DateTime<Utc>) -> Approval {
        let needs_ask = match call.risk {
            Risk::Read => false,
            Risk::Write => call.outside_workspace,
            Risk::External => !call.host_allowed,
            Risk::Destructive => true,
        };
        if !needs_ask {
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

    /// Convenience for callers that only know the risk class.
    pub fn decide_risk(&self, risk: Risk, approvals: &Approvals, now: DateTime<Utc>) -> Approval {
        self.decide(
            CallContext {
                risk,
                outside_workspace: false,
                host_allowed: false,
            },
            approvals,
            now,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_never_ask() {
        let p = Policy;
        let a = Approvals::default();
        assert_eq!(p.decide_risk(Risk::Read, &a, Utc::now()), Approval::Allow);
    }

    #[test]
    fn destructive_asks_then_respects_not_now() {
        let p = Policy;
        let mut a = Approvals::default();
        let now = Utc::now();
        assert_eq!(p.decide_risk(Risk::Destructive, &a, now), Approval::Ask);
        a.deny(now);
        assert_eq!(
            p.decide_risk(Risk::Destructive, &a, now + Duration::minutes(1)),
            Approval::Deny
        );
        assert_eq!(
            p.decide_risk(Risk::Destructive, &a, now + Duration::minutes(31)),
            Approval::Ask
        );
    }

    #[test]
    fn grants_cover_the_conversation() {
        let p = Policy;
        let mut a = Approvals::default();
        a.grant();
        assert_eq!(
            p.decide_risk(Risk::Destructive, &a, Utc::now()),
            Approval::Allow
        );
    }

    #[test]
    fn write_inside_is_allowed_outside_asks() {
        let p = Policy;
        let a = Approvals::default();
        let now = Utc::now();
        let inside = CallContext {
            risk: Risk::Write,
            outside_workspace: false,
            host_allowed: false,
        };
        let outside = CallContext {
            risk: Risk::Write,
            outside_workspace: true,
            host_allowed: false,
        };
        assert_eq!(p.decide(inside, &a, now), Approval::Allow);
        assert_eq!(p.decide(outside, &a, now), Approval::Ask);
    }

    #[test]
    fn external_asks_unless_the_host_is_allowlisted() {
        let p = Policy;
        let a = Approvals::default();
        let now = Utc::now();
        let unknown_host = CallContext {
            risk: Risk::External,
            outside_workspace: false,
            host_allowed: false,
        };
        let allowlisted_host = CallContext {
            risk: Risk::External,
            outside_workspace: false,
            host_allowed: true,
        };
        assert_eq!(p.decide(unknown_host, &a, now), Approval::Ask);
        assert_eq!(p.decide(allowlisted_host, &a, now), Approval::Allow);
    }

    #[test]
    fn a_url_carrying_data_is_not_a_reason_to_allow_it() {
        // The old matrix tried to distinguish a fetch from an upload by URL
        // shape. Both directions now ask, so no shape heuristic can be fooled.
        let p = Policy;
        let a = Approvals::default();
        let now = Utc::now();
        for url in [
            "https://example.com/docs",
            "https://collector.test/?d=screenshot-text",
        ] {
            assert!(crate::tools::net::parse(url).is_ok());
            assert_eq!(
                p.decide(
                    CallContext {
                        risk: Risk::External,
                        outside_workspace: false,
                        host_allowed: crate::tools::net::host_allowed(url, &[]),
                    },
                    &a,
                    now
                ),
                Approval::Ask,
                "{url}"
            );
        }
    }
}
