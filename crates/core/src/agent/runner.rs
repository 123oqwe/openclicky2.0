//! The task loop: context → model → policy → tool → checkpoint.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use serde_json::json;

use crate::agent::{transition, Checkpoint, PendingCall, Task, TaskState};
use crate::error::{Error, Result};
use crate::memory::Memory;
use crate::ports::{ChatMessage, ChatProvider, ChatRequest, Clock, Role};
use crate::store::{StepRecord, Store, TaskRow, TaskStateUpdate};
use crate::tools::policy::{Approval, Approvals, Policy, Risk};
use crate::tools::{Registry, ToolCtx, ToolOutcome};
use crate::usage::{MeterInput, UsageKind, UsageMeter};

pub struct AgentDeps {
    pub store: Arc<Store>,
    pub chat: Arc<dyn ChatProvider>,
    pub tools: Arc<Registry>,
    pub clock: Arc<dyn Clock>,
    pub usage: Arc<UsageMeter>,
    pub memory: Arc<Memory>,
    pub http: Arc<dyn crate::ports::HttpTransport>,
    pub commands: Arc<dyn crate::ports::CommandRunner>,
    pub permissions: Arc<dyn crate::ports::PermissionProbe>,
    pub workspace: std::path::PathBuf,
    pub model: String,
    pub max_steps: u32,
    pub allow_private_network: bool,
    pub shell_enabled: bool,
    pub events: Option<Arc<dyn Fn(crate::AppEvent) + Send + Sync>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepOutcome {
    pub state: TaskState,
    pub tool: Option<String>,
    pub message: String,
}

/// How many tasks may be queued/running/waiting at the same time.
pub const MAX_ACTIVE_TASKS: usize = 3;

/// A waiting task that has sat for this long is parked instead of holding the
/// queue slot forever.
const WAITING_TIMEOUT: Duration = Duration::hours(24);

pub struct TaskRunner {
    deps: Arc<AgentDeps>,
}

pub struct CancelToken {
    flag: AtomicBool,
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            flag: AtomicBool::new(false),
        }
    }
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskRunner {
    pub fn new(deps: Arc<AgentDeps>) -> Self {
        Self { deps }
    }

    fn active_task_count(&self) -> Result<usize> {
        let rows = self.deps.store.list_tasks(None)?;
        Ok(rows
            .iter()
            .filter(|r| {
                TaskState::parse(&r.state)
                    .map(|s| s.is_active())
                    .unwrap_or(false)
            })
            .count())
    }

    fn emit(&self, e: crate::AppEvent) {
        if let Some(f) = &self.deps.events {
            f(e);
        }
    }

    pub fn create(&self, goal: &str) -> Result<Task> {
        let goal = goal.trim();
        if goal.is_empty() {
            return Err(Error::BadRequest("任务目标不能为空".into()));
        }
        // At most MAX_ACTIVE tasks in flight. Without this the queue grows
        // without bound and every one of them spends budget (INV-8).
        if self.active_task_count()? >= MAX_ACTIVE_TASKS {
            return Err(Error::QueueFull);
        }
        let now = self.deps.clock.now();
        let task = Task {
            id: uuid::Uuid::new_v4().to_string(),
            name: Task::short_name(goal),
            goal: goal.to_string(),
            state: TaskState::Queued,
            checkpoint: Checkpoint::default(),
            created_at: now,
            updated_at: now,
            epoch: 1,
            units: 0.0,
            steps_done: 0,
            result: None,
            error_code: None,
        };
        self.deps.store.upsert_task(&to_row(&task))?;
        let ws = self.deps.workspace.join("tasks").join(&task.id);
        std::fs::create_dir_all(&ws).ok();
        self.emit(crate::AppEvent::TaskUpdated {
            id: task.id.clone(),
            state: task.state,
            message: "已创建".into(),
        });
        Ok(task)
    }

    pub fn get(&self, id: &str) -> Result<Task> {
        self.deps
            .store
            .task(id)?
            .map(from_row)
            .ok_or_else(|| Error::BadRequest(format!("任务不存在：{id}")))
    }

    pub fn tool_ctx(&self, task: &Task) -> ToolCtx {
        self.tool_ctx_with(task, false)
    }

    pub fn tool_ctx_with(&self, task: &Task, allow_outside_write: bool) -> ToolCtx {
        ToolCtx {
            workspace: self.deps.workspace.join("tasks").join(&task.id),
            allow_outside_write,
            task_id: task.id.clone(),
            http: self.deps.http.clone(),
            clock: self.deps.clock.clone(),
            commands: self.deps.commands.clone(),
            allow_private_network: self.deps.allow_private_network,
            shell_enabled: self.deps.shell_enabled,
        }
    }

    fn persist(
        &self,
        task: &mut Task,
        next: TaskState,
        result: Option<String>,
        code: Option<String>,
    ) -> Result<()> {
        let next = transition(task.state, next)?;
        task.state = next;
        task.updated_at = self.deps.clock.now();
        if result.is_some() {
            task.result = result;
        }
        if code.is_some() {
            task.error_code = code;
        }
        let row = to_row(task);
        let update = TaskStateUpdate {
            task_id: row.id.clone(),
            state: row.state.clone(),
            checkpoint_json: row.checkpoint_json.clone(),
            heartbeat_at: Some(self.deps.clock.now()),
            units: Some(row.units),
            steps_done: Some(row.steps_done),
            result: row.result.clone(),
            error_code: row.error_code.clone(),
            expected_epoch: row.epoch,
        };
        self.deps.store.commit_turn(&crate::store::Turn {
            session_id: String::new(),
            messages: Vec::new(),
            usage: None,
            task_update: Some(update),
        })?;
        // Record (or clear) the waiting marker in the same step as the state
        // change, so the watchdog can expire tasks nobody answers.
        let waiting = matches!(
            task.state,
            TaskState::WaitingApproval | TaskState::WaitingUser
        );
        if waiting {
            // Best effort: a lost epoch means another runner owns the row now.
            let _ = self
                .deps
                .store
                .mark_waiting(&task.id, task.epoch, self.deps.clock.now());
        } else {
            self.deps.store.clear_waiting(&task.id)?;
        }
        self.emit(crate::AppEvent::TaskUpdated {
            id: task.id.clone(),
            state: task.state,
            message: task.result.clone().unwrap_or_default(),
        });
        Ok(())
    }

    /// Advance exactly one step. Deterministic and directly testable.
    pub fn step(&self, task_id: &str) -> Result<StepOutcome> {
        let mut task = self.get(task_id)?;

        if task.state == TaskState::Queued {
            self.persist(&mut task, TaskState::Running, None, None)?;
            // The task-level charge happens once, at start (SDD §4.8).
            let start_now = self.deps.clock.now();
            let units = self.deps.usage.meter(&MeterInput {
                kind: UsageKind::AgentTask,
                task_id: Some(task.id.clone()),
                session_id: None,
                started_at: task.created_at,
                now: start_now,
                task_already_charged: false,
            })?;
            task.units += units;
        } else if !matches!(
            task.state,
            TaskState::Running | TaskState::WaitingApproval | TaskState::WaitingUser
        ) {
            return Err(Error::State(format!(
                "任务处于 {} 状态，不能执行",
                task.state.as_str()
            )));
        }

        if task.steps_done as u32 >= self.deps.max_steps {
            let msg = format!("步数超过上限 {}", self.deps.max_steps);
            self.persist(
                &mut task,
                TaskState::Failed,
                Some(msg.clone()),
                Some("too_many_steps".into()),
            )?;
            return Ok(StepOutcome {
                state: task.state,
                tool: None,
                message: msg,
            });
        }

        // A pending approval must be resolved before anything else runs.
        if let Some(pending) = task.checkpoint.pending_call.clone() {
            return self.resume_pending(&mut task, pending);
        }

        let now = self.deps.clock.now();
        let budget_ok = self.deps.usage.can_start(now)?;
        if !budget_ok {
            let msg = "预算用完，任务暂停".to_string();
            self.persist(
                &mut task,
                TaskState::Paused,
                Some(msg.clone()),
                Some("budget".into()),
            )?;
            return Ok(StepOutcome {
                state: task.state,
                tool: None,
                message: msg,
            });
        }

        let request = self.build_request(&task)?;
        let response = self.deps.chat.complete(&request)?;

        // Charge one agent turn; long-running continuation is waived.
        let elapsed_long = now.signed_duration_since(task.created_at) > Duration::minutes(20);
        let units = self.deps.usage.meter(&MeterInput {
            kind: UsageKind::AgentStep,
            task_id: Some(task.id.clone()),
            session_id: None,
            started_at: task.created_at,
            now,
            task_already_charged: elapsed_long && task.units > 0.0,
        })?;
        task.units += units;
        task.steps_done += 1;

        if response.tool_calls.is_empty() {
            let text = if response.text.trim().is_empty() {
                "模型没有给出结果".to_string()
            } else {
                response.text
            };
            self.deps.store.record_step(&StepRecord {
                task_id: task.id.clone(),
                step: task.steps_done,
                kind: "answer".into(),
                tool: None,
                request_json: None,
                result_json: Some(json!({ "text": text }).to_string()),
                ok: true,
                code: None,
                created_at: now,
            })?;
            let msg = text.clone();
            self.persist(
                &mut task,
                TaskState::CompletedUnverified,
                Some(text),
                Some("no_evidence".into()),
            )?;
            return Ok(StepOutcome {
                state: task.state,
                tool: None,
                message: msg,
            });
        }

        let mut last_tool = None;
        for call in response.tool_calls {
            let outcome = self.execute(&mut task, call)?;
            last_tool = Some(outcome.tool.clone().unwrap_or_default());
            if !matches!(task.state, TaskState::Running) {
                return Ok(outcome);
            }
        }

        self.persist(&mut task, TaskState::Running, None, None)?;
        Ok(StepOutcome {
            state: task.state,
            tool: last_tool,
            message: format!("步 {} 完成", task.steps_done),
        })
    }

    fn resume_pending(&self, task: &mut Task, pending: PendingCall) -> Result<StepOutcome> {
        let risk = self.deps.tools.risk_of(&pending.name, &pending.args);
        let now = self.deps.clock.now();
        let approvals = self.approvals_from(task, &pending.name, &pending.args);
        let call = self.call_context(task, &pending.name, &pending.args, risk);
        let decision = Policy.decide(call, &approvals, now);
        match decision {
            Approval::Allow => {
                task.checkpoint.pending_call = None;
                let name = pending.name.clone();
                let approved_outside = call.outside_workspace;
                // An approval is one-shot: drop the grant before running so a
                // failure cannot leave the permission lying around.
                consume_grant(task, &pending.name, &pending.args);
                let ctx = self.tool_ctx_with(task, approved_outside);
                let outcome = self.run_tool(task, &ctx, &pending)?;
                self.after_tool(task, name, outcome)
            }
            Approval::Deny => {
                task.checkpoint.pending_call = None;
                self.deps.store.record_step(&StepRecord {
                    task_id: task.id.clone(),
                    step: task.steps_done,
                    kind: "denied".into(),
                    tool: Some(pending.name.clone()),
                    request_json: None,
                    result_json: None,
                    ok: false,
                    code: Some("policy".into()),
                    created_at: now,
                })?;
                self.persist(task, TaskState::Running, None, None)?;
                Ok(StepOutcome {
                    state: task.state,
                    tool: Some(pending.name),
                    message: "用户拒绝，已改道".into(),
                })
            }
            Approval::Ask => Ok(StepOutcome {
                state: task.state,
                tool: Some(pending.name),
                message: "等待批准".into(),
            }),
        }
    }

    fn call_context(
        &self,
        task: &Task,
        name: &str,
        args: &serde_json::Value,
        risk: Risk,
    ) -> crate::tools::policy::CallContext {
        let workspace = self.deps.workspace.join("tasks").join(&task.id);
        let (outside, upload) = match self.deps.tools.get(name) {
            Some(t) => (
                t.outside_workspace(args, &workspace),
                t.uploads_local_file(args),
            ),
            None => (false, false),
        };
        crate::tools::policy::CallContext {
            risk,
            outside_workspace: outside,
            uploads_local_file: upload,
        }
    }

    /// Approval state for exactly one call. A grant only unblocks the tool and
    /// arguments it was given for; an expired `Not now` window is cleaned up
    /// here so the checkpoint cannot grow stale denials forever.
    fn approvals_from(&self, task: &mut Task, name: &str, args: &serde_json::Value) -> Approvals {
        let now = self.deps.clock.now();
        if let Some(until) = task.checkpoint.denied_until {
            if now >= until {
                task.checkpoint.denied_until = None;
            }
        }
        let key = grant_key(name, args);
        Approvals {
            conversation_granted: task.checkpoint.actions.iter().any(|x| x == &key),
            denied_until: task.checkpoint.denied_until,
        }
    }

    fn execute(&self, task: &mut Task, call: crate::ports::ToolCall) -> Result<StepOutcome> {
        let name = call.name.clone();
        let args = call.arguments.clone();

        if name == "finish" {
            return self.finish(task, &args);
        }
        if name == "ask_user" {
            let q = args
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("需要你的确认")
                .to_string();
            task.checkpoint.pending_question = Some(q.clone());
            self.persist(task, TaskState::WaitingUser, None, None)?;
            return Ok(StepOutcome {
                state: task.state,
                tool: Some(name),
                message: q,
            });
        }

        if self.deps.tools.get(&name).is_none() {
            return self.after_tool(
                task,
                name.clone(),
                ToolOutcome::Failed {
                    code: "unknown_tool".into(),
                    message: name,
                },
            );
        }

        let risk = self.deps.tools.risk_of(&name, &args);

        // Re-check the OS permissions before every state-changing call (INV-7).
        if !matches!(risk, Risk::Read) && !self.deps.permissions.accessibility() {
            task.checkpoint.pending_call = Some(PendingCall {
                name: name.clone(),
                args,
            });
            self.persist(
                task,
                TaskState::WaitingApproval,
                None,
                Some("permission".into()),
            )?;
            return Ok(StepOutcome {
                state: task.state,
                tool: Some(name),
                message: "辅助功能权限已被撤销，等待恢复".into(),
            });
        }

        let now = self.deps.clock.now();
        let approvals = self.approvals_from(task, &name, &args);
        let call = self.call_context(task, &name, &args, risk);
        let decision = Policy.decide(call, &approvals, now);
        match decision {
            Approval::Allow => {
                // Consume the one-shot grant as soon as it is used.
                consume_grant(task, &name, &args);
            }
            Approval::Deny => {
                // A denied attempt still consumed a turn: number it so the step
                // log stays unique, and skip the call without running it.
                task.steps_done += 1;
                task.checkpoint.step = task.steps_done as u32;
                let reason = "用户之前选择了不要再问（30 分钟内）";
                self.deps.store.record_step(&StepRecord {
                    task_id: task.id.clone(),
                    step: task.steps_done,
                    kind: "denied".into(),
                    tool: Some(name.clone()),
                    request_json: Some(crate::security::log_safe(&args.to_string(), 200)),
                    result_json: None,
                    ok: false,
                    code: Some("policy".into()),
                    created_at: now,
                })?;
                task.checkpoint
                    .facts
                    .push(format!("{name} 被拒绝：{reason}"));
                if task.checkpoint.facts.len() > 20 {
                    let drain = task.checkpoint.facts.len() - 20;
                    task.checkpoint.facts.drain(0..drain);
                }
                self.persist(task, TaskState::Running, None, None)?;
                return Ok(StepOutcome {
                    state: task.state,
                    tool: Some(name),
                    message: reason.to_string(),
                });
            }
            Approval::Ask => {
                task.checkpoint.pending_call = Some(PendingCall {
                    name: name.clone(),
                    args,
                });
                self.persist(task, TaskState::WaitingApproval, None, None)?;
                self.emit(crate::AppEvent::ApprovalRequest {
                    id: task.id.clone(),
                    tool: name.clone(),
                    risk,
                });
                return Ok(StepOutcome {
                    state: task.state,
                    tool: Some(name),
                    message: "等待批准".into(),
                });
            }
        }

        if self.is_loop(task, &name, &args)? {
            let msg = "检测到重复调用，已停止".to_string();
            self.persist(
                task,
                TaskState::Failed,
                Some(msg.clone()),
                Some("loop".into()),
            )?;
            return Ok(StepOutcome {
                state: task.state,
                tool: Some(name),
                message: msg,
            });
        }

        let ctx = self.tool_ctx(task);
        let digest = crate::store::args_digest(&args);
        let outcome = self.run_tool(
            task,
            &ctx,
            &PendingCall {
                name: name.clone(),
                args,
            },
        )?;
        self.after_tool_with(task, name, outcome, Some(digest))
    }

    fn run_tool(&self, task: &Task, ctx: &ToolCtx, call: &PendingCall) -> Result<ToolOutcome> {
        let Some(tool) = self.deps.tools.get(&call.name) else {
            return Ok(ToolOutcome::Failed {
                code: "unknown_tool".into(),
                message: call.name.clone(),
            });
        };
        let risk = tool.risk(&call.args);
        let outcome = match tool.run(ctx, &call.args) {
            Ok(o) => o,
            Err(e) => ToolOutcome::Failed {
                code: e.code().to_string(),
                message: e.user_message(),
            },
        };
        self.deps.store.audit(&crate::store::AuditRow {
            task_id: Some(task.id.clone()),
            tool: call.name.clone(),
            risk: risk.as_str().to_string(),
            args_digest: crate::store::args_digest(&call.args),
            outcome: match &outcome {
                ToolOutcome::Ok { .. } => "ok".into(),
                ToolOutcome::Denied { .. } => "denied".into(),
                ToolOutcome::Failed { code, .. } => format!("failed:{code}"),
            },
            created_at: self.deps.clock.now(),
        })?;
        Ok(outcome)
    }

    fn after_tool(
        &self,
        task: &mut Task,
        name: String,
        outcome: ToolOutcome,
    ) -> Result<StepOutcome> {
        self.after_tool_with(task, name, outcome, None)
    }

    fn after_tool_with(
        &self,
        task: &mut Task,
        name: String,
        outcome: ToolOutcome,
        digest: Option<String>,
    ) -> Result<StepOutcome> {
        let now = self.deps.clock.now();
        let (ok, code, summary) = match &outcome {
            ToolOutcome::Ok { summary, .. } => (true, None, summary.clone()),
            ToolOutcome::Denied { reason } => (false, Some("denied".to_string()), reason.clone()),
            ToolOutcome::Failed { code, message } => (false, Some(code.clone()), message.clone()),
        };
        self.deps.store.record_step(&StepRecord {
            task_id: task.id.clone(),
            step: task.steps_done,
            kind: "tool".into(),
            tool: Some(name.clone()),
            request_json: digest.clone(),
            result_json: Some(crate::security::log_safe(&summary, 400)),
            ok,
            code: code.clone(),
            created_at: now,
        })?;
        let fenced = crate::prompt::wrap_tool_result(&name, &summary);
        if !ok {
            task.checkpoint.facts.push(format!("{name} 失败：{fenced}"));
        } else {
            task.checkpoint.facts.push(format!("{name} 成功：{fenced}"));
            if name == "fs_delete" || name == "shell" {
                task.checkpoint.actions.push(name.clone());
            }
        }
        // Keep facts bounded so the context does not grow without limit.
        if task.checkpoint.facts.len() > 20 {
            let drain = task.checkpoint.facts.len() - 20;
            task.checkpoint.facts.drain(0..drain);
        }
        task.checkpoint.step = task.steps_done as u32;
        self.persist(task, TaskState::Running, None, None)?;
        Ok(StepOutcome {
            state: task.state,
            tool: Some(name),
            message: summary,
        })
    }

    fn finish(&self, task: &mut Task, args: &serde_json::Value) -> Result<StepOutcome> {
        let result = args
            .get("result")
            .and_then(|v| v.as_str())
            .unwrap_or("完成")
            .to_string();
        let artifacts: Vec<String> = args
            .get("artifacts")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let ctx = self.tool_ctx(task);
        let mut verified = 0;
        for a in &artifacts {
            if crate::tools::fs::resolve_in_workspace(&ctx.workspace, a)
                .map(|p| p.exists())
                .unwrap_or(false)
            {
                verified += 1;
            }
        }
        let (state, code) = if !artifacts.is_empty() && verified == artifacts.len() {
            (TaskState::Completed, None)
        } else {
            (
                TaskState::CompletedUnverified,
                Some("no_evidence".to_string()),
            )
        };
        let msg = result.clone();
        self.persist(task, state, Some(result), code)?;
        Ok(StepOutcome {
            state: task.state,
            tool: Some("finish".into()),
            message: msg,
        })
    }

    fn is_loop(&self, task: &Task, name: &str, args: &serde_json::Value) -> Result<bool> {
        let digest = crate::store::args_digest(args);
        let steps = self.deps.store.steps(&task.id)?;
        let count = steps
            .iter()
            .filter(|s| {
                s.tool.as_deref() == Some(name)
                    && s.request_json.as_deref() == Some(digest.as_str())
            })
            .count();
        // Two prior identical calls mean this attempt is the third: refuse it.
        Ok(count >= 2)
    }

    fn build_request(&self, task: &Task) -> Result<ChatRequest> {
        let system = self.agent_system();
        let mut facts = String::new();
        for f in &task.checkpoint.facts {
            facts.push_str("- ");
            facts.push_str(f);
            facts.push('\n');
        }
        let pending = task
            .checkpoint
            .pending_question
            .as_ref()
            .map(|q| format!("\nthe user answered your question: {q}\n"))
            .unwrap_or_default();
        let user = format!(
            "goal: {}\n\nwhat already happened:\n{}{}\ncontinue. when you are done, call finish with a short result and any artifacts.",
            task.goal,
            if facts.is_empty() { "- nothing yet\n" } else { &facts },
            pending
        );
        Ok(ChatRequest {
            model: self.deps.model.clone(),
            system,
            messages: vec![ChatMessage {
                role: Role::User,
                content: user,
            }],
            images: Vec::new(),
            tools: self.deps.tools.specs(),
            max_tokens: 1500,
            temperature: 0.2,
        })
    }

    fn agent_system(&self) -> String {
        let mut s = String::new();
        s.push_str(
            "you are openclicky running a task for the user on their own computer.\n\
             you act only through the provided tools. never claim something happened unless a tool result proves it.\n\
             if you need a decision, ask exactly one small question.\n\
             keep going until the goal is met, then call finish with artifacts you actually created.\n",
        );
        let profile = self.deps.memory.profile_text();
        if !profile.is_empty() {
            s.push_str("\nuser preferences:\n");
            s.push_str(&profile);
        }
        let projects = self.deps.memory.projects_text();
        if !projects.is_empty() {
            s.push_str("\nongoing context:\n");
            s.push_str(&projects);
        }
        s.push_str("\ntools available: ");
        let names: Vec<String> = self
            .deps
            .tools
            .specs()
            .into_iter()
            .map(|t| t.name)
            .collect();
        s.push_str(&names.join(", "));
        s
    }

    /// Approve exactly the call that is waiting. The grant is keyed by tool and
    /// argument digest and removed once that call has run.
    pub fn approve(&self, task_id: &str) -> Result<()> {
        let mut task = self.get(task_id)?;
        if let Some(pending) = task.checkpoint.pending_call.clone() {
            let key = grant_key(&pending.name, &pending.args);
            if !task.checkpoint.actions.iter().any(|x| x == &key) {
                task.checkpoint.actions.push(key);
            }
        }
        task.checkpoint.denied_until = None;
        if task.state == TaskState::WaitingApproval {
            task.state = transitioned(TaskState::WaitingApproval, TaskState::Running);
        }
        task.updated_at = self.deps.clock.now();
        self.deps.store.upsert_task(&to_row(&task))?;
        Ok(())
    }

    /// `Not now`: never ask again for 30 minutes; the pending call is dropped.
    pub fn deny(&self, task_id: &str) -> Result<()> {
        let mut task = self.get(task_id)?;
        task.checkpoint.pending_call = None;
        task.checkpoint.denied_until = Some(self.deps.clock.now() + Duration::minutes(30));
        task.checkpoint
            .facts
            .push("用户选择了 Not now（30 分钟内不再询问）".into());
        if task.state == TaskState::WaitingApproval {
            task.state = transitioned(TaskState::WaitingApproval, TaskState::Running);
        }
        task.updated_at = self.deps.clock.now();
        self.deps.store.upsert_task(&to_row(&task))?;
        Ok(())
    }

    pub fn cancel(&self, task_id: &str) -> Result<()> {
        let mut task = self.get(task_id)?;
        if !task.state.is_terminal() {
            self.persist(&mut task, TaskState::Canceled, Some("已取消".into()), None)?;
        }
        Ok(())
    }

    /// Retry/resume: bump epoch so any stale runner loses write ownership.
    pub fn resume(&self, task_id: &str) -> Result<Task> {
        let mut task = self.get(task_id)?;
        if task.state.is_terminal() && task.state != TaskState::Failed {
            return Ok(task);
        }
        task.epoch += 1;
        let next = match task.state {
            TaskState::Queued => TaskState::Running,
            TaskState::Paused
            | TaskState::Failed
            | TaskState::WaitingApproval
            | TaskState::WaitingUser => TaskState::Running,
            other => other,
        };
        task.state = next;
        task.updated_at = self.deps.clock.now();
        self.deps.store.upsert_task(&to_row(&task))?;
        Ok(task)
    }

    /// Watchdog: a task that has been waiting for a human for too long is
    /// parked instead of holding a queue slot forever.
    pub fn expire_stale_waiting(&self, now: DateTime<Utc>) -> Result<Vec<String>> {
        let cutoff = now - WAITING_TIMEOUT;
        let mut expired = Vec::new();
        for row in self.deps.store.stale_waiting(cutoff)? {
            let mut task = from_row(row);
            task.epoch += 1;
            task.state = TaskState::Paused;
            task.error_code = Some("waiting_timeout".into());
            task.result = Some("等太久了，任务已暂停。需要的话重新开始。".into());
            task.updated_at = now;
            self.deps.store.upsert_task(&to_row(&task))?;
            self.deps.store.clear_waiting(&task.id)?;
            self.emit(crate::AppEvent::TaskUpdated {
                id: task.id.clone(),
                state: task.state,
                message: task.result.clone().unwrap_or_default(),
            });
            expired.push(task.id);
        }
        Ok(expired)
    }

    /// Startup recovery: any running task without a recent heartbeat is paused.
    pub fn recover_interrupted(&self, now: DateTime<Utc>) -> Result<usize> {
        let mut recovered = 0;
        for row in self.deps.store.list_tasks(Some("running"))? {
            let stale = match row.heartbeat_at {
                Some(h) => now.signed_duration_since(h) > Duration::minutes(20),
                None => now.signed_duration_since(row.updated_at) > Duration::minutes(20),
            };
            if stale {
                let mut task = from_row(row);
                task.epoch += 1;
                task.state = TaskState::Paused;
                task.error_code = Some("interrupted".into());
                task.result = Some("上次运行被中断，可以重试".into());
                task.updated_at = now;
                self.deps.store.upsert_task(&to_row(&task))?;
                recovered += 1;
            }
        }
        Ok(recovered)
    }

    /// Run until terminal, cancelled, or waiting for the user.
    pub fn run(&self, task_id: &str, cancel: &CancelToken) -> Result<Task> {
        loop {
            if cancel.is_cancelled() {
                self.cancel(task_id)?;
                return self.get(task_id);
            }
            let outcome = self.step(task_id)?;
            if outcome.state.is_terminal()
                || matches!(
                    outcome.state,
                    TaskState::WaitingUser | TaskState::WaitingApproval
                )
            {
                return self.get(task_id);
            }
        }
    }

    pub fn answer_question(&self, task_id: &str, answer: &str) -> Result<()> {
        let mut task = self.get(task_id)?;
        task.checkpoint.facts.push(format!(
            "用户回答：{}",
            crate::prompt::wrap_user_answer(&crate::security::log_safe(answer, 300))
        ));
        task.checkpoint.pending_question = None;
        task.state = transition(task.state, TaskState::Running)?;
        task.updated_at = self.deps.clock.now();
        self.deps.store.upsert_task(&to_row(&task))?;
        Ok(())
    }
}

/// One-shot approval key: this tool with exactly these arguments.
fn grant_key(name: &str, args: &serde_json::Value) -> String {
    format!("grant:{name}:{}", crate::store::args_digest(args))
}

fn consume_grant(task: &mut Task, name: &str, args: &serde_json::Value) {
    let key = grant_key(name, args);
    task.checkpoint
        .actions
        .retain(|x| x != "approved" && x != &key);
}

fn transitioned(from: TaskState, to: TaskState) -> TaskState {
    transition(from, to).unwrap_or(from)
}

pub fn to_row(t: &Task) -> TaskRow {
    TaskRow {
        id: t.id.clone(),
        name: t.name.clone(),
        goal: t.goal.clone(),
        state: t.state.as_str().to_string(),
        checkpoint_json: serde_json::to_string(&t.checkpoint).unwrap_or_else(|_| "{}".into()),
        created_at: t.created_at,
        updated_at: t.updated_at,
        heartbeat_at: Some(t.updated_at),
        epoch: t.epoch,
        units: t.units,
        steps_done: t.steps_done,
        result: t.result.clone(),
        error_code: t.error_code.clone(),
        owner: "local".into(),
        retries: 0,
        prompt_version: Some(crate::prompt::PROMPT_VERSION.to_string()),
        tool_schema_version: Some("tools-1".to_string()),
        waiting_since: None,
    }
}

pub fn from_row(r: TaskRow) -> Task {
    Task {
        id: r.id,
        name: r.name,
        goal: r.goal,
        state: TaskState::parse(&r.state).unwrap_or(TaskState::Failed),
        checkpoint: serde_json::from_str(&r.checkpoint_json).unwrap_or_default(),
        created_at: r.created_at,
        updated_at: r.updated_at,
        epoch: r.epoch,
        units: r.units,
        steps_done: r.steps_done,
        result: r.result,
        error_code: r.error_code,
    }
}
