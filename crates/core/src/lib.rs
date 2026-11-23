//! OpenClicky core: all product rules, state machines and algorithms.
//!
//! This crate is deliberately free of GUI, network and platform dependencies so
//! that every rule can be tested offline and deterministically. The outside
//! world enters only through the traits in [`ports`].

pub mod agent;
pub mod annotations;
pub mod clock;
pub mod config;
pub mod deps;
pub mod dictation;
pub mod error;
pub mod hotkey;
pub mod memory;
pub mod ports;
pub mod privacy;
pub mod prompt;
pub mod router;
pub mod routines;
pub mod security;
pub mod store;
pub mod suggest;
pub mod tools;
pub mod usage;
pub mod voice;
pub mod walkthrough;

pub use error::{Error, Permission, Result};

/// Event stream emitted by the orchestrator towards the UI shell.
#[derive(Debug, Clone, PartialEq)]
pub enum AppEvent {
    State(voice::State),
    Transcript {
        text: String,
        partial: bool,
    },
    Delta {
        text: String,
    },
    Answer {
        text: String,
    },
    Annotations {
        plan_id: String,
    },
    Subtitle {
        text: String,
    },
    TaskUpdated {
        id: String,
        state: agent::TaskState,
        message: String,
    },
    ApprovalRequest {
        id: String,
        tool: String,
        risk: tools::Risk,
    },
    Usage {
        percent: f64,
    },
    Notice {
        code: String,
        message: String,
    },
    Dictation {
        text: String,
        partial: bool,
    },
}
