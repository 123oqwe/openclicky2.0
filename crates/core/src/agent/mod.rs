//! Background tasks: one task state machine owns execution and recovery.

pub mod runner;

pub use runner::{AgentDeps, StepOutcome, TaskRunner};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    Queued,
    Running,
    WaitingUser,
    WaitingApproval,
    Completed,
    CompletedUnverified,
    Failed,
    Paused,
    Canceled,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskState::Queued => "queued",
            TaskState::Running => "running",
            TaskState::WaitingUser => "waiting_user",
            TaskState::WaitingApproval => "waiting_approval",
            TaskState::Completed => "completed",
            TaskState::CompletedUnverified => "completed_unverified",
            TaskState::Failed => "failed",
            TaskState::Paused => "paused",
            TaskState::Canceled => "canceled",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "queued" => TaskState::Queued,
            "running" => TaskState::Running,
            "waiting_user" => TaskState::WaitingUser,
            "waiting_approval" => TaskState::WaitingApproval,
            "completed" => TaskState::Completed,
            "completed_unverified" => TaskState::CompletedUnverified,
            "failed" => TaskState::Failed,
            "paused" => TaskState::Paused,
            "canceled" => TaskState::Canceled,
            other => return Err(Error::State(format!("unknown task state: {other}"))),
        })
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskState::Completed
                | TaskState::CompletedUnverified
                | TaskState::Failed
                | TaskState::Canceled
        )
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            TaskState::Queued
                | TaskState::Running
                | TaskState::WaitingUser
                | TaskState::WaitingApproval
        )
    }
}

/// The only legal transitions (docs/02-sdd.md §11).
pub fn transition(from: TaskState, to: TaskState) -> Result<TaskState> {
    use TaskState::*;
    let ok = match (from, to) {
        (Queued, Running) | (Queued, Canceled) => true,
        (Running, Running) => true,
        (Running, WaitingApproval) | (Running, WaitingUser) => true,
        (Running, Completed) | (Running, CompletedUnverified) => true,
        (Running, Failed) | (Running, Paused) | (Running, Canceled) => true,
        (WaitingApproval, Running) | (WaitingApproval, Failed) | (WaitingApproval, Canceled) => {
            true
        }
        (WaitingApproval, Completed) | (WaitingApproval, CompletedUnverified) => true,
        (WaitingUser, Running) | (WaitingUser, Paused) | (WaitingUser, Canceled) => true,
        (Paused, Running) | (Paused, Canceled) => true,
        (Failed, Running) | (Failed, Canceled) => true,
        (a, b) if a == b => true,
        _ => false,
    };
    if ok {
        Ok(to)
    } else {
        Err(Error::State(format!(
            "非法任务状态转换 {} → {}",
            from.as_str(),
            to.as_str()
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PendingCall {
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub version: u32,
    pub step: u32,
    /// The goal restated in the model's own words, re-read every step.
    pub goal_notes: String,
    pub facts: Vec<String>,
    pub pending_question: Option<String>,
    pub pending_call: Option<PendingCall>,
    /// Non-idempotent actions already executed; never replayed.
    pub actions: Vec<String>,
}

impl Default for Checkpoint {
    fn default() -> Self {
        Self {
            version: 1,
            step: 0,
            goal_notes: String::new(),
            facts: Vec::new(),
            pending_question: None,
            pending_call: None,
            actions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub goal: String,
    pub state: TaskState,
    pub checkpoint: Checkpoint,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub epoch: i64,
    pub units: f64,
    pub steps_done: i64,
    pub result: Option<String>,
    pub error_code: Option<String>,
}

impl Task {
    pub fn short_name(goal: &str) -> String {
        let words: Vec<&str> = goal.split_whitespace().take(3).collect();
        let name = words.join(" ");
        let name = name.chars().take(28).collect::<String>();
        if name.trim().is_empty() {
            "task".to_string()
        } else {
            name
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_states_are_terminal() {
        assert!(TaskState::Completed.is_terminal());
        assert!(TaskState::Failed.is_terminal());
        assert!(TaskState::Canceled.is_terminal());
        assert!(!TaskState::Running.is_terminal());
    }

    #[test]
    fn transitions_follow_the_table() {
        assert!(transition(TaskState::Queued, TaskState::Running).is_ok());
        assert!(transition(TaskState::Running, TaskState::Completed).is_ok());
        assert!(transition(TaskState::Running, TaskState::WaitingApproval).is_ok());
        assert!(transition(TaskState::Paused, TaskState::Running).is_ok());
        assert!(transition(TaskState::Completed, TaskState::Running).is_err());
        assert!(transition(TaskState::Canceled, TaskState::Running).is_err());
    }

    #[test]
    fn short_name_is_bounded() {
        let n = Task::short_name("把这两周的邮件按客户打标签并整理成一个表格文件");
        assert!(n.chars().count() <= 28);
    }
}
