//! L3: the task loop with a scripted model. Each test asserts state, tool
//! effects and evidence, not prose.

mod support;

use openclicky_core::agent::runner::AgentDeps;
use openclicky_core::agent::{TaskRunner, TaskState};
use openclicky_core::ports::{ChatResponse, FinishReason, Spend, ToolCall};
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
    clock: Arc<FakeClock>,
    chat: Arc<ScriptedChat>,
}

fn script(responses: Vec<ChatResponse>) -> Ctx {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    script_with(store, dir, responses)
}

fn script_on_disk(responses: Vec<ChatResponse>) -> Ctx {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(&dir.path().join("app.db")).unwrap());
    script_with(store, dir, responses)
}

/// A harness whose memory files already hold something, the state the app is in
/// on the second launch after `openclicky remember` wrote them.
fn script_with_memory(responses: Vec<ChatResponse>, prefs: &[&str]) -> Ctx {
    let dir = tempfile::tempdir().unwrap();
    let mem_dir = dir.path().join("memory");
    std::fs::create_dir_all(&mem_dir).unwrap();
    std::fs::write(mem_dir.join("profile.md"), prefs.join("\n")).unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    script_with(store, dir, responses)
}

fn script_with(store: Arc<Store>, dir: tempfile::TempDir, responses: Vec<ChatResponse>) -> Ctx {
    let chat = ScriptedChat::new(responses);
    let chat_handle = chat.clone();
    // Deterministic time: the approval windows are minutes wide, so tests move
    // the clock instead of sleeping.
    let fake_clock = FakeClock::new();
    let clock: Arc<dyn openclicky_core::ports::Clock> = fake_clock.clone();
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
        // The test workspace is not the app's config; the protected-path tests pass
        // their own list to prove the guard, these do not need one.
        protected: Vec::new(),
        model: "fake-model".into(),
        max_steps: 30,
        allow_private_network: false,
        allow_domains: Vec::new(),
        shell_enabled: false,
        events: None,
    });
    Ctx {
        runner: TaskRunner::new(deps),
        _dir: dir,
        store,
        probe,
        clock: fake_clock,
        chat: chat_handle,
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
        usage: Spend {
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

    let artifacts = c.store.artifacts_for_task(&task.id).unwrap();
    assert_eq!(artifacts.len(), 1, "{artifacts:?}");
    assert_eq!(artifacts[0].path, "notes/out.txt");
    assert_eq!(artifacts[0].kind, "txt");
    assert_eq!(artifacts[0].bytes, 5);
    assert_eq!(
        artifacts[0].sha256,
        openclicky_core::security::sha256_hex(b"hello")
    );
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
    assert_eq!(
        c.store.pending_approvals().unwrap().len(),
        1,
        "a waiting call is a durable ledger row, not only checkpoint state"
    );
    c.runner.approve(&task.id).unwrap();
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.tool.as_deref(), Some("fs_delete"));
    let t = c.runner.get(&task.id).unwrap();
    assert!(!c.runner.tool_ctx(&t).workspace.join("b.txt").exists());
    assert!(
        c.store.pending_approvals().unwrap().is_empty(),
        "the granted call is no longer pending"
    );
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
fn a_key_in_tool_arguments_never_reaches_the_database() {
    let key = "sk-live-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let c = script_on_disk(vec![
        tool_call("fs_write", json!({"path":"k.txt","content": key})),
        tool_call("finish", json!({"result":"wrote the key file"})),
    ]);
    let task = c.runner.create("把我的 key 存成一个文件").unwrap();
    c.runner.step(&task.id).unwrap();
    c.runner.step(&task.id).unwrap();

    let path = c.store.path().unwrap().to_path_buf();
    c.store.close().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        contains(&bytes, b"k.txt"),
        "control: the step summary must be in the database"
    );
    assert!(
        !contains(&bytes, key.as_bytes()),
        "INV-2: the key reached the database file"
    );

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }
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

    let steps_before = c.store.steps(&task.id).unwrap().len();
    let recovered = c.runner.recover_interrupted(chrono::Utc::now()).unwrap();
    assert_eq!(recovered, 1);
    let t = c.runner.get(&task.id).unwrap();
    assert_eq!(t.state, TaskState::Paused);
    assert_eq!(t.error_code.as_deref(), Some("interrupted"));
    assert_eq!(
        c.store.steps(&task.id).unwrap().len(),
        steps_before,
        "INV-8: recovery must not replay the interrupted call"
    );
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

    let audit = c.store.audit_rows(&task.id).unwrap();
    assert!(
        audit
            .iter()
            .any(|r| r.tool == "fs_write" && r.outcome == "ok" && r.risk == "write"),
        "INV-5: an approved write outside the workspace must be audited, got {audit:?}"
    );
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

#[test]
fn task_is_charged_once_at_start_and_steps_separately() {
    let c = script(vec![
        tool_call("fs_write", json!({"path":"bill.txt","content":"x"})),
        tool_call("finish", json!({"result":"done"})),
    ]);
    let task = c.runner.create("计费").unwrap();
    c.runner.step(&task.id).unwrap();
    let now = chrono::Utc::now();
    let rows = c
        .store
        .usage_between(
            now - chrono::Duration::hours(1),
            now + chrono::Duration::hours(1),
        )
        .unwrap();
    let kinds: Vec<&str> = rows.iter().map(|r| r.kind.as_str()).collect();
    assert_eq!(
        kinds.iter().filter(|k| **k == "agent_task").count(),
        1,
        "task fee once"
    );
    assert_eq!(
        kinds.iter().filter(|k| **k == "agent_step").count(),
        1,
        "one step charged"
    );

    c.runner.step(&task.id).unwrap();
    let rows = c
        .store
        .usage_between(
            now - chrono::Duration::hours(1),
            now + chrono::Duration::hours(1),
        )
        .unwrap();
    let kinds: Vec<&str> = rows.iter().map(|r| r.kind.as_str()).collect();
    assert_eq!(
        kinds.iter().filter(|k| **k == "agent_task").count(),
        1,
        "INV-6: the task fee is charged once, not once per step"
    );
    assert_eq!(
        kinds.iter().filter(|k| **k == "agent_step").count(),
        2,
        "INV-6: each finished step is charged exactly once"
    );
}

// ---- approval / queue / waiting contracts ----

#[test]
fn approval_is_one_shot_for_the_same_call() {
    let c = script(vec![
        tool_call("fs_write", json!({"path":"once.txt","content":"x"})),
        tool_call("fs_delete", json!({"path":"once.txt"})),
        tool_call("fs_delete", json!({"path":"once.txt"})),
    ]);
    let task = c.runner.create("删一次").unwrap();
    c.runner.step(&task.id).unwrap();
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.state, TaskState::WaitingApproval);

    c.runner.approve(&task.id).unwrap();
    let t = c.runner.get(&task.id).unwrap();
    assert!(
        t.checkpoint
            .actions
            .iter()
            .any(|a| a.starts_with("grant:fs_delete:")),
        "the approval is a grant for this exact call: {:?}",
        t.checkpoint.actions
    );

    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.tool.as_deref(), Some("fs_delete"));

    // The grant is gone once the call has run, so the very same dangerous call
    // has to ask again.
    let t = c.runner.get(&task.id).unwrap();
    assert!(
        t.checkpoint
            .actions
            .iter()
            .all(|a| !a.starts_with("grant:")),
        "the grant must be consumed: {:?}",
        t.checkpoint.actions
    );
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(
        outcome.state,
        TaskState::WaitingApproval,
        "the second identical delete must ask again"
    );
}

#[test]
fn not_now_blocks_asking_for_thirty_minutes() {
    let c = script(vec![
        tool_call("fs_write", json!({"path":"n.txt","content":"x"})),
        tool_call("fs_delete", json!({"path":"n.txt"})),
        tool_call("fs_delete", json!({"path":"n.txt"})),
        tool_call("fs_delete", json!({"path":"n.txt"})),
    ]);
    let task = c.runner.create("删文件").unwrap();
    c.runner.step(&task.id).unwrap();
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.state, TaskState::WaitingApproval);

    c.runner.deny(&task.id).unwrap();
    let t = c.runner.get(&task.id).unwrap();
    assert!(
        t.checkpoint.denied_until.is_some(),
        "deny has to record a 30 minute window"
    );

    // Right afterwards the same call is denied instead of prompting again.
    let outcome = c.runner.step(&task.id).unwrap();
    assert_ne!(
        outcome.state,
        TaskState::WaitingApproval,
        "a denied window must not ask again"
    );
    assert!(
        c.store
            .steps(&task.id)
            .unwrap()
            .iter()
            .any(|s| s.kind == "denied"),
        "the call must be recorded as denied"
    );

    // 31 minutes later the window is over and the user is asked again.
    c.clock.advance(chrono::Duration::minutes(31));
    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.state, TaskState::WaitingApproval);
    let t = c.runner.get(&task.id).unwrap();
    assert!(
        t.checkpoint.denied_until.is_none(),
        "an expired window is cleared"
    );
}

#[test]
fn waiting_state_is_time_stamped_and_cleared() {
    let c = script(vec![
        tool_call("fs_write", json!({"path":"w.txt","content":"x"})),
        tool_call("fs_delete", json!({"path":"w.txt"})),
    ]);
    let task = c.runner.create("写文件").unwrap();
    c.runner.step(&task.id).unwrap();
    assert!(
        c.store
            .task(&task.id)
            .unwrap()
            .unwrap()
            .waiting_since
            .is_none(),
        "a running task is not waiting"
    );

    let outcome = c.runner.step(&task.id).unwrap();
    assert_eq!(outcome.state, TaskState::WaitingApproval);
    assert!(
        c.store
            .task(&task.id)
            .unwrap()
            .unwrap()
            .waiting_since
            .is_some(),
        "waiting for approval must be time stamped"
    );

    c.runner.approve(&task.id).unwrap();
    c.runner.step(&task.id).unwrap();
    assert!(
        c.store
            .task(&task.id)
            .unwrap()
            .unwrap()
            .waiting_since
            .is_none(),
        "leaving the waiting state clears the marker"
    );
}

#[test]
fn stale_waiting_tasks_are_paused_by_the_watchdog() {
    let c = script(vec![
        tool_call("fs_write", json!({"path":"s.txt","content":"x"})),
        tool_call("fs_delete", json!({"path":"s.txt"})),
    ]);
    let task = c.runner.create("写文件").unwrap();
    c.runner.step(&task.id).unwrap();
    c.runner.step(&task.id).unwrap();

    let now = c.clock.now_utc();
    assert!(
        c.runner.expire_stale_waiting(now).unwrap().is_empty(),
        "a fresh wait is left alone"
    );

    let expired = c
        .runner
        .expire_stale_waiting(now + chrono::Duration::hours(25))
        .unwrap();
    assert_eq!(expired, vec![task.id.clone()]);
    let t = c.runner.get(&task.id).unwrap();
    assert_eq!(t.state, TaskState::Paused);
    assert_eq!(t.error_code.as_deref(), Some("waiting_timeout"));
    assert!(c
        .store
        .task(&task.id)
        .unwrap()
        .unwrap()
        .waiting_since
        .is_none());
}

#[test]
fn the_fourth_active_task_is_rejected() {
    let c = script(vec![]);
    for i in 0..3 {
        c.runner.create(&format!("第{i}件事")).unwrap();
    }
    let err = c.runner.create("第四件事").unwrap_err();
    assert_eq!(err.code(), "E_QUEUE");
}

#[test]
fn a_tool_result_reaches_the_model_as_content_not_a_digest() {
    let body = "quarterly revenue was 42 units and the margin held";
    let c = script(vec![
        tool_call("fs_write", json!({"path":"notes.txt","content": body})),
        tool_call("fs_read", json!({"path":"notes.txt"})),
        tool_call("finish", json!({"result":"summarised"})),
    ]);
    let task = c
        .runner
        .create("read my note and tell me what it says")
        .unwrap();
    c.runner.step(&task.id).unwrap();
    c.runner.step(&task.id).unwrap();
    c.runner.step(&task.id).unwrap();

    let request = c
        .chat
        .last_request()
        .expect("a third turn must have happened");
    let rendered = request
        .messages
        .iter()
        .map(|m| m.content.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("quarterly revenue was 42 units"),
        "the model must receive the file body, not a char count; got: {rendered}"
    );
}

#[test]
fn a_remembered_preference_reaches_the_task_prompt() {
    // The write side is the `openclicky remember` command; this pins the read
    // side, which is the half that feeds the model.
    let c = script_with_memory(
        vec![tool_call("finish", json!({"result": "done"}))],
        &["回答用中文，短一点"],
    );

    let task = c.runner.create("说点什么").unwrap();
    c.runner.step(&task.id).unwrap();

    let request = c.chat.last_request().expect("a turn must have happened");
    assert!(
        request.system.contains("回答用中文，短一点"),
        "a remembered preference belongs in the system prompt; got: {}",
        request.system
    );
}

#[test]
fn an_unreadable_state_is_an_error_not_a_silent_failure() {
    let c = script(vec![tool_call("finish", json!({"result": "done"}))]);
    let task = c.runner.create("做点什么").unwrap();
    let mut row = c.store.task(&task.id).unwrap().unwrap();
    row.state = "banana".into();
    c.store.upsert_task(&row).unwrap();

    let err = c.runner.get(&task.id).unwrap_err();
    assert!(
        err.user_message().contains("banana") || err.user_message().contains("状态"),
        "a corrupt row must say what is wrong, got: {}",
        err.user_message()
    );
}

#[test]
fn an_unreadable_row_still_counts_against_the_queue() {
    let c = script(vec![tool_call("finish", json!({"result": "done"}))]);
    let mut stuck = vec![];
    for i in 0..3 {
        let t = c.runner.create(&format!("任务 {i}")).unwrap();
        let mut row = c.store.task(&t.id).unwrap().unwrap();
        row.state = "banana".into();
        c.store.upsert_task(&row).unwrap();
        stuck.push(t.id);
    }
    let err = c.runner.create("第四个").unwrap_err();
    assert_eq!(
        err.code(),
        "E_QUEUE",
        "three unreadable rows must not free the queue"
    );
}

#[test]
fn an_unreadable_row_can_be_released_so_the_queue_frees_up() {
    let c = script(vec![tool_call("finish", json!({"result": "done"}))]);
    for i in 0..3 {
        let t = c.runner.create(&format!("任务 {i}")).unwrap();
        let mut row = c.store.task(&t.id).unwrap().unwrap();
        row.state = "banana".into();
        c.store.upsert_task(&row).unwrap();
    }
    assert!(
        c.runner.create("第四个").is_err(),
        "the three rows must hold their slots first"
    );

    let released = c.runner.recover_interrupted(chrono::Utc::now()).unwrap();
    assert_eq!(released, 3, "each unreadable row is parked once");
    assert!(
        c.runner.create("现在可以了").is_ok(),
        "a parked row must not keep its slot"
    );
}

#[test]
fn a_question_waiting_for_an_answer_is_not_fed_back_as_the_answer() {
    let c = script(vec![
        tool_call("ask_user", json!({"question": "删掉哪个文件？"})),
        tool_call("finish", json!({"result": "done"})),
    ]);
    let task = c.runner.create("清理文件").unwrap();
    let first = c.runner.step(&task.id).unwrap();
    assert_eq!(first.state, TaskState::WaitingUser);

    // A retry or a watchdog can step a WaitingUser task again without an answer.
    let _ = c.runner.step(&task.id);

    let request = c.chat.last_request().expect("a turn must have happened");
    let rendered = request
        .messages
        .iter()
        .map(|m| m.content.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !rendered.contains("the user answered your question"),
        "an unanswered question must not be labelled as the answer; got: {rendered}"
    );
    assert!(
        rendered.contains("删掉哪个文件"),
        "the question itself stays visible"
    );
}

#[test]
fn a_parked_row_becomes_readable_again() {
    let c = script(vec![tool_call("finish", json!({"result": "done"}))]);
    let t = c.runner.create("会坏掉的任务").unwrap();
    let mut row = c.store.task(&t.id).unwrap().unwrap();
    row.checkpoint_json = "{not json".into();
    c.store.upsert_task(&row).unwrap();

    c.runner.recover_interrupted(chrono::Utc::now()).unwrap();

    let repaired = c
        .runner
        .get(&t.id)
        .expect("a parked row must be readable, otherwise nothing can show it");
    assert_eq!(repaired.state, TaskState::Paused);
}

#[test]
fn a_finished_task_keeps_its_state_even_with_a_broken_checkpoint() {
    let c = script(vec![tool_call("finish", json!({"result": "done"}))]);
    let t = c.runner.create("已经完成的任务").unwrap();
    c.runner.step(&t.id).unwrap();
    let mut row = c.store.task(&t.id).unwrap().unwrap();
    row.checkpoint_json = "{not json".into();
    c.store.upsert_task(&row).unwrap();

    let released = c.runner.recover_interrupted(chrono::Utc::now()).unwrap();
    assert_eq!(released, 0, "a finished task is not holding a slot");
    assert_eq!(
        c.store.task(&t.id).unwrap().unwrap().state,
        "completed_unverified",
        "history must not be rewritten to paused"
    );
}

#[test]
fn an_approved_outside_write_still_cannot_touch_the_apps_own_files() {
    use openclicky_core::tools::{fs, FsWrite, Tool, ToolCtx};

    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("config");
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();
    let config_file = config_dir.join("config.toml");
    std::fs::write(&config_file, "base_url = \"https://api.openai.com/v1\"\n").unwrap();
    let db = data_dir.join("app.db");
    std::fs::write(&db, b"sqlite").unwrap();
    let secrets = data_dir.join("secrets.enc");
    std::fs::write(&secrets, b"key").unwrap();

    let ctx = ToolCtx {
        workspace: dir.path().join("workspace"),
        // The user said yes to writing outside the workspace. That approval is
        // exactly what used to reach these files.
        allow_outside_write: true,
        protected: vec![
            config_file.clone(),
            db.clone(),
            data_dir.join("memory"),
            secrets.clone(),
        ],
        task_id: "t1".into(),
        http: ScriptedHttp::new(vec![]),
        clock: FakeClock::new(),
        commands: FakeCommands::new(),
        allow_private_network: false,
        shell_enabled: false,
    };

    for target in [&config_file, &db, &secrets] {
        let err = FsWrite
            .run(
                &ctx,
                &json!({ "path": target.to_string_lossy(), "content": "pwned" }),
            )
            .expect_err("a protected file must be refused even with approval");
        assert_eq!(
            err.code(),
            "E_POLICY",
            "{} — {}",
            target.display(),
            err.user_message()
        );
        assert!(
            !std::fs::read_to_string(target).unwrap().contains("pwned"),
            "{} was overwritten",
            target.display()
        );
    }
    assert!(fs::protected_hit(&config_file, &ctx.protected).is_some());
}
