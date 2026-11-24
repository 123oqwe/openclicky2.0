//! L3: the task loop with a scripted model. Each test asserts state, tool
//! effects and evidence, not prose.

mod support;

use openclicky_core::agent::runner::AgentDeps;
use openclicky_core::agent::{TaskRunner, TaskState};
use openclicky_core::ports::{ChatResponse, FinishReason, TokenUsage, ToolCall};
use openclicky_core::store::Store;
use openclicky_core::tools::Registry;
use openclicky_core::usage::UsageMeter;
use serde_json::json;
use std::sync::Arc;
use support::*;

struct Ctx {
    runner: TaskRunner,
    _dir: tempfile::TempDir,
    store: Arc<Store>,
    probe: Arc<FakeProbe>,
}

fn script(responses: Vec<ChatResponse>) -> Ctx {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let chat = ScriptedChat::new(responses);
    let clock: Arc<dyn openclicky_core::ports::Clock> =
        Arc::new(openclicky_core::clock::SystemClock);
    let usage = Arc::new(UsageMeter::new(
        store.clone(),
        clock.clone(),
        Default::default(),
        1000.0,
        10000.0,
    ));
    let memory = Arc::new(
        openclicky_core::memory::Memory::load(dir.path().join("memory").as_path()).unwrap(),
    );
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let probe = FakeProbe::allowing();
    let deps = Arc::new(AgentDeps {
        store: store.clone(),
        chat,
        tools: Arc::new(Registry::default_set()),
        clock,
        usage,
        memory,
        http: ScriptedHttp::new(vec![]),
        commands: FakeCommands::new(),
        permissions: probe.clone(),
        workspace,
        model: "fake-model".into(),
        max_steps: 30,
        allow_private_network: false,
        shell_enabled: false,
        events: None,
    });
    Ctx {
        runner: TaskRunner::new(deps),
        _dir: dir,
        store,
        probe,
    }
}

fn tool_call(name: &str, args: serde_json::Value) -> ChatResponse {
    ChatResponse {
        text: String::new(),
        tool_calls: vec![ToolCall {
            id: "c1".into(),
            name: name.into(),
            arguments: args,
        }],
        usage: TokenUsage {
            input_tokens: 10,
            output_tokens: 5,
        },
        finish: FinishReason::ToolCalls,
    }
}

#[test]
fn writes_a_file_then_finishes_with_evidence() {
    let c = script(vec![
        tool_call(
            "fs_write",
            json!({"path":"notes/out.txt","content":"hello"}),
        ),
        tool_call(
            "finish",
            json!({"result":"wrote notes/out.txt","artifacts":["notes/out.txt"]}),
        ),
    ]);
    let task = c.runner.create("写一个笔记文件").unwrap();
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.state, TaskState::Running);
    assert_eq!(outcome.tool.as_deref(), Some("fs_write"));

    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.state, TaskState::Completed);

    let t = c.runner.get(&task.id).unwrap();
    assert_eq!(t.state, TaskState::Completed);
    let path = c.runner.tool_ctx(&t).workspace.join("notes/out.txt");
    assert!(path.exists());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "hello");
}

#[test]
fn claim_without_artifact_is_unverified() {
    let c = script(vec![tool_call("finish", json!({"result":"done"}))]);
    let task = c.runner.create("做点什么").unwrap();
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.state, TaskState::CompletedUnverified);
    let t = c.runner.get(&task.id).unwrap();
    assert_eq!(t.error_code.as_deref(), Some("no_evidence"));
}

#[test]
fn destructive_tool_waits_for_approval_and_denial_prevents_execution() {
    let c = script(vec![
        tool_call("fs_write", json!({"path":"a.txt","content":"x"})),
        tool_call("fs_delete", json!({"path":"a.txt"})),
        tool_call("finish", json!({"result":"cleaned"})),
    ]);
    let task = c.runner.create("删掉临时文件").unwrap();
    c.runner.step(&task.id).unwrap();
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.state, TaskState::WaitingApproval);

    // Not now: the file must still exist.
    c.runner.deny(&task.id).unwrap();
    let t = c.runner.get(&task.id).unwrap();
    assert!(c.runner.tool_ctx(&t).workspace.join("a.txt").exists());

    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.state, TaskState::CompletedUnverified);
}

#[test]
fn approval_lets_the_action_run_once() {
    let c = script(vec![
        tool_call("fs_write", json!({"path":"b.txt","content":"x"})),
        tool_call("fs_delete", json!({"path":"b.txt"})),
        tool_call("finish", json!({"result":"cleaned"})),
    ]);
    let task = c.runner.create("删掉临时文件").unwrap();
    c.runner.step(&task.id).unwrap();
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.state, TaskState::WaitingApproval);
    c.runner.approve(&task.id).unwrap();
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.tool.as_deref(), Some("fs_delete"));
    let t = c.runner.get(&task.id).unwrap();
    assert!(!c.runner.tool_ctx(&t).workspace.join("b.txt").exists());
}

#[test]
fn repeated_identical_calls_stop_the_task() {
    let same = json!({"path":"loop.txt","content":"x"});
    let c = script(vec![
        tool_call("fs_read", same.clone()),
        tool_call("fs_read", same.clone()),
        tool_call("fs_read", same.clone()),
    ]);
    let task = c.runner.create("读一个文件").unwrap();
    c.runner.step(&task.id).unwrap();
    c.runner.step(&task.id).unwrap();
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.state, TaskState::Failed);
    assert_eq!(
        c.runner.get(&task.id).unwrap().error_code.as_deref(),
        Some("loop")
    );
}

#[test]
fn cancellation_is_terminal_and_keeps_the_checkpoint() {
    let c = script(vec![tool_call(
        "fs_write",
        json!({"path":"c.txt","content":"x"}),
    )]);
    let task = c.runner.create("写文件").unwrap();
    c.runner.step(&task.id).unwrap();
    c.runner.cancel(&task.id).unwrap();
    let t = c.runner.get(&task.id).unwrap();
    assert_eq!(t.state, TaskState::Canceled);
    assert_eq!(t.checkpoint.step, 1);
}

#[test]
fn startup_recovery_pauses_stale_running_tasks() {
    let c = script(vec![tool_call(
        "fs_write",
        json!({"path":"d.txt","content":"x"}),
    )]);
    let task = c.runner.create("写文件").unwrap();
    c.runner.step(&task.id).unwrap();

    // Move the heartbeat far into the past.
    let mut row = c.store.task(&task.id).unwrap().unwrap();
    row.heartbeat_at = Some(chrono::Utc::now() - chrono::Duration::minutes(45));
    c.store.upsert_task(&row).unwrap();

    let recovered = c.runner.recover_interrupted(chrono::Utc::now()).unwrap();
    assert_eq!(recovered, 1);
    let t = c.runner.get(&task.id).unwrap();
    assert_eq!(t.state, TaskState::Paused);
    assert_eq!(t.error_code.as_deref(), Some("interrupted"));
}

#[test]
fn shell_is_denied_when_disabled() {
    let c = script(vec![
        tool_call("shell", json!({"command":"echo hi"})),
        tool_call("finish", json!({"result":"done"})),
    ]);
    let task = c.runner.create("跑个命令").unwrap();
    let outcome = c.runner.step(&task.id).unwrap();
    // Destructive risk -> asks first.
    assert_eq!(outcome.state, TaskState::WaitingApproval);
    c.runner.approve(&task.id).unwrap();
    let outcome = c.runner.step(&task.id).unwrap();
    assert!(outcome.message.contains("disabled") || outcome.message.contains("shell"));
}

// ---- F3 red tests: these describe the contract, not the current behaviour ----

#[test]
fn write_outside_workspace_asks_instead_of_failing() {
    // SDD §11 / tech-prd SEC-4: Write(outside workspace) => WaitingApproval.
    // Today the sandbox rejects it with a tool failure, so this is red.
    let c = script(vec![
        tool_call("fs_write", json!({"path":"../escape.txt","content":"x"})),
        tool_call("finish", json!({"result":"done"})),
    ]);
    let task = c.runner.create("写到工作区外").unwrap();
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.state, TaskState::WaitingApproval);
}

#[test]
fn approving_an_outside_write_lets_it_through_once() {
    let c = script(vec![
        tool_call("fs_write", json!({"path":"../approved.txt","content":"ok"})),
        tool_call("finish", json!({"result":"done"})),
    ]);
    let task = c.runner.create("批准后写到工作区外").unwrap();
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.state, TaskState::WaitingApproval);
    let t = c.runner.get(&task.id).unwrap();
    let target = c.runner.tool_ctx(&t).workspace.join("../approved.txt");
    assert!(!target.exists(), "nothing may be written before approval");

    c.runner.approve(&task.id).unwrap();
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.tool.as_deref(), Some("fs_write"));
    assert!(target.exists(), "approved write should land");
}

#[test]
fn revoked_permission_blocks_state_changing_tools() {
    let c = script(vec![
        tool_call("fs_write", json!({"path":"blocked.txt","content":"x"})),
        tool_call("finish", json!({"result":"done"})),
    ]);
    let task = c.runner.create("写文件").unwrap();
    c.probe.deny_accessibility();
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.state, TaskState::WaitingApproval);
    let t = c.runner.get(&task.id).unwrap();
    assert!(!c.runner.tool_ctx(&t).workspace.join("blocked.txt").exists());
}
