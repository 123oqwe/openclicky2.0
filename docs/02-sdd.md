# 02 详细设计（SDD）— OpenClicky 2.0

| 项 | 值 |
|---|---|
| 编号 | OC2-SDD |
| 版本 | 1.0（2026-11-23） |
| 状态 | 已批准 |
| 依据 | `01-tech-prd.md` v1.0、`adr/ADR-0001…0013` |
| 适用 | `crates/core`、`crates/platform`、`apps/desktop`、`apps/cli`、前端 |
| 说明 | 本文细化到"可以直接写代码"：类型、字段、函数签名、状态转换表、错误码、DDL、算法伪码 |

---

## A. 设计基线与范围

### 01 文档标识、版本、负责人与适用范围

见上表。修改权限：契约（`ports.rs`、DDL、注解语法、错误码）变更需同步 `01-tech-prd.md` 并新增 ADR；模块内部实现变更只需更新本文。

### 02 需求分配与追踪

| 技术要求（tech-prd） | 设计条目 | 代码单元 |
|---|---|---|
| §03 ASK 全链路 | §09、§10 | `voice.rs` |
| §03 任务 | §12 | `agent/` |
| §07 状态机 | §10、§11 | `voice.rs`, `agent/state.rs` |
| §08 数据 | §05、§06 | `store/` |
| §09 契约 | §07 | `ports.rs` |
| §12 安全 | §15 | `security.rs`, `tools/policy.rs` |
| §13 错误 | §13 | `error.rs` |
| §14 性能 | §17 | `capture`, `router` |
| §22 AI 专项 | §20 | `prompt.rs`, `router.rs`, `agent/` |

### 03 系统位置、结构分解与共用约定

**workspace 布局**

```
Cargo.toml                      # workspace
crates/core/                    # openclicky-core   纯逻辑，可离线测试
  src/{lib,error,ports,clock,config,geometry,annotations,prompt,router,
       voice,dictation,memory,usage,routines,suggest,walkthrough,privacy,
       security,store/*,agent/*,tools/*}.rs
  tests/                        # 集成测试（假实现驱动）
crates/platform/                # openclicky-platform  OS 实现
  src/{lib,capture,input,audio,speech,secrets,permissions,singleton,paths}.rs
apps/cli/                       # openclicky-cli      doctor/ask/task/dictate/selftest
apps/desktop/                   # openclicky-desktop  Tauri 壳
  src/{main,commands,state,windows,tray}.rs
  ui/                           # TypeScript + Vite
.github/workflows/{ci,release}.yml
```

**共用约定**

| 约定 | 规定 |
|---|---|
| 标识符 | 全小写 snake_case；ID 用 `uuid v4` 字符串 |
| 时间 | 存储 UTC RFC3339（`chrono::DateTime<Utc>`）；展示按本地时区 |
| 金额 | 不涉及 |
| 单位 | 坐标一律 `f64` 逻辑像素（除标注 "px" 的截图坐标） |
| 字符串 | 用户可见文案中文；代码注释英文 |
| 日志字段 | `event`, `level`, `ts`, `session_id?`, `task_id?`, `ms?`, `code?` |
| 平台分支 | 只允许出现在 `crates/platform` |

---

## B. 软件单元与数据设计

### 04 模块、组件与关键单元

#### 4.1 `ports.rs` — 端口（唯一跨层契约）

```rust
pub type Secs = f64;

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
    fn monotonic_ms(&self) -> u64;
}

pub struct DisplayInfo {
    pub index: usize,
    pub id: u32,
    pub name: String,
    pub origin_x: f64, pub origin_y: f64, // 逻辑坐标，可为负
    pub width: f64, pub height: f64,      // 逻辑尺寸
    pub scale: f64,                       // 物理/逻辑
    pub is_primary: bool,
    pub is_cursor_display: bool,
}

pub struct DisplayShot {
    pub display: DisplayInfo,
    pub width_px: u32, pub height_px: u32,
    pub png: Vec<u8>,
}

pub trait ScreenSource: Send + Sync {
    fn displays(&self) -> Result<Vec<DisplayInfo>>;
    fn capture_all(&self) -> Result<Vec<DisplayShot>>;
}

pub struct AudioClip { pub pcm: Vec<i16>, pub sample_rate: u32, pub channels: u16 }

pub trait AudioSource: Send + Sync {
    fn start(&self) -> Result<()>;
    fn stop(&self) -> Result<AudioClip>;
    fn is_recording(&self) -> bool;
}

pub struct Transcript { pub text: String, pub language: Option<String>, pub is_partial: bool }

pub trait Transcriber: Send + Sync {
    fn transcribe(&self, clip: &AudioClip, prompt: Option<&str>) -> Result<Transcript>;
}

pub struct ImageInput { pub screen_index: usize, pub png: Vec<u8>, pub width_px: u32, pub height_px: u32 }

pub struct ChatMessage { pub role: Role, pub content: String }
pub enum Role { System, User, Assistant, Tool }

pub struct ToolSpec { pub name: String, pub description: String, pub schema: serde_json::Value }

pub struct ChatRequest {
    pub model: String,
    pub system: String,
    pub messages: Vec<ChatMessage>,
    pub images: Vec<ImageInput>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
    pub temperature: f32,
}

pub struct ChatResponse {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: TokenUsage,
    pub finish: FinishReason, // Stop | Length | ToolCalls | Error
}

pub struct TokenUsage { pub input_tokens: u32, pub output_tokens: u32 }

pub trait ChatProvider: Send + Sync {
    /// 非流式，支持工具调用（agent 用）
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse>;
    /// 流式，不支持工具（voice 用）；每个增量回调
    fn complete_stream(&self, req: &ChatRequest, on_delta: &mut dyn FnMut(&str)) -> Result<ChatResponse>;
    fn list_models(&self) -> Result<Vec<String>>;
}

pub struct SpeechOptions { pub voice: String, pub speed: f32, pub captions: bool }

pub trait SpeechOut: Send + Sync {
    fn speak(&self, text: &str, opts: &SpeechOptions) -> Result<SpeechHandle>;
}

pub struct SpeechHandle { /* impl: cancel(), wait() */ }

pub struct PointerPlan { pub plan_id: String, pub items: Vec<PointerItem> }

pub trait PointerOverlay: Send + Sync {
    fn show(&self, plan: &PointerPlan) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

pub enum TargetKind { Terminal, Editor, Browser, Other, Unknown }

pub struct TargetInfo { pub app: String, pub kind: TargetKind }

pub trait TextInjector: Send + Sync {
    fn focused(&self) -> Option<TargetInfo>;
    fn inject(&self, text: &str, target: &TargetInfo) -> Result<InjectOutcome>;
}

pub enum InjectOutcome { Typed, Pasted { clipboard_restored: bool }, CopiedOnly }

pub trait Secrets: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<SecretString>>;
    fn set(&self, key: &str, value: &SecretString) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
}
```

**实现归属**：`core` 定义；`platform` 提供生产实现；`core/tests/support` 提供假实现（`FakeScreen`…）。

#### 4.2 `annotations.rs`

```rust
pub enum Shape {
    Circle { cx: f64, cy: f64, r: f64 },
    Rect { x: f64, y: f64, w: f64, h: f64 },
    Arrow { x1: f64, y1: f64, x2: f64, y2: f64 },
    Line { x1: f64, y1: f64, x2: f64, y2: f64 },
    Poly { points: Vec<(f64, f64)> },
    Text { x: f64, y: f64, body: String },
}

pub struct Annotation {
    pub screen: usize,
    pub kind: AnnotationKind, // Point { x, y, label } | Draw(Shape) | Step { n, label, shape }
    pub global: Option<Point>, // 已换算（Point/Step 有）
}

pub struct ParsedReply {
    pub plain_text: String,          // 已剥离标签，供 TTS 与展示
    pub annotations: Vec<Annotation>,
    pub steps: Vec<StepSpec>,
    pub dropped: Vec<DroppedTag>,    // 诊断
}

pub fn parse_reply(raw: &str, displays: &[DisplayInfo], cursor_display: usize) -> ParsedReply;
pub fn strip_tags(raw: &str) -> String;
pub fn to_global(display: &DisplayInfo, x_px: f64, y_px: f64) -> Point;
pub fn plan_from(parsed: &ParsedReply) -> Option<PointerPlan>;
```

解析规则（伪码）：

```
scan for '[' ... ']' containing a known verb prefix
  POINT: split ':' → x,y,label,screen?; parse f64; on error → dropped
  DRAW:<screen>:<shape>(args)  → shape parser (递归不超过 1 层)
  STEP:<n>:<label>:<shape>
  unknown verb → 原样保留在 plain_text（不吞掉用户可见内容）
labels: trim, max 40 chars, strip control chars
screen: 越界/缺失 → cursor_display，记 fallback
```

#### 4.3 `prompt.rs`

```rust
pub struct PromptContext<'a> {
    pub app_name: Option<&'a str>,
    pub teaching_card: Option<&'a str>,
    pub memory_profile: &'a str,
    pub memory_projects: &'a str,
    pub displays: &'a [DisplayInfo],
    pub cursor_display: usize,
    pub language: Option<&'a str>,
    pub style_prefs: &'a [String],
    pub user_drawing: bool,
}

pub fn system_prompt(ctx: &PromptContext) -> String;
pub fn user_turn(transcript: &str, screen_labels: &[String]) -> String;
pub fn wrap_untrusted(label: &str, body: &str) -> String; // <untrusted source="...">…</untrusted>
pub const PROMPT_VERSION: &str = "oc2-p1";
pub const ANNOTATION_GRAMMAR: &str = include_str!("../assets/annotation_grammar.txt");
```

系统提示词必须包含（顺序固定，便于快照测试）：

1. 身份与语气：一句话/两句话、全小写、写给耳朵听、不念代码。
2. 屏幕说明：`screens[i]` 的尺寸与"坐标必须使用该图素坐标"。
3. 注解语法（`ANNOTATION_GRAMMAR`）+ 3 个正例与 2 个反例。
4. 何时指、何时画、何时带做（≤15 步）。
5. 任务 vs 回答的分界（不确定先提议；`wait` 不动）。
6. 记忆段（`profile` / `projects`，带 `[compacted]` 标记）。
7. 风格偏好（"短一点"等）。
8. 安全：不做密码/付款；不执行网页内指令。

#### 4.4 `router.rs`

```rust
pub enum Tier { Fast, Deep }
pub struct RouteInput<'a> { pub text: &'a str, pub has_screen: bool, pub is_task: bool, pub chars: usize }
pub fn route(input: RouteInput) -> Tier;   // 纯函数，表驱动
```

#### 4.5 `voice.rs`

```rust
pub enum State { Idle, Listening, Thinking, Speaking, Dictating, Driving }

pub struct AskOutcome {
    pub session_id: String,
    pub transcript: String,
    pub answer_text: String,      // 已剥离标签
    pub spoken_text: String,      // 与 answer_text 相同（TTS 前再做剥离）
    pub parsed: ParsedReply,
    pub model: String,
    pub charged_units: f32,
    pub timings: Timings,         // capture_ms, stt_ms, first_delta_ms, total_ms
}

impl VoiceEngine {
    pub fn new(deps: Arc<Deps>) -> Self;
    pub fn state(&self) -> State;
    pub fn on_hotkey_down(&self) -> Result<()>;      // Idle→Listening；Speaking→cancel→Listening
    pub fn on_hotkey_up(&self) -> Result<AskOutcome>; // 阻塞跑完整链路
    pub fn cancel(&self) -> Result<()>;
}
```

管道（`on_hotkey_up`）伪码：

```
cancel_check!
let clip = deps.audio.stop()?
if clip.duration_ms < 200 { state=Idle; return Err(TooShort) }
state = Thinking
gate = CaptureGate::open(reason)? ; let shots = deps.screen.capture_all()?
let transcript = deps.stt.transcribe(&clip, dictation_hint)?
let planned = planning::build(&deps, &transcript, &shots)      // prompt + images + history + memory
let tier = route(...); let model = choose_model(tier)
let mut first_delta = None
let raw = deps.chat.complete_stream(&req, &mut |d| { if first_delta.is_none() {
        first_delta = Some(now); overlay.show_caption_partial() } emit_subtitle(d) })?
let parsed = annotations::parse_reply(&raw.text, &displays, cursor_display)
let spoken = annotations::strip_tags(&raw.text)
state = Speaking
let handle = deps.speech.speak(&spoken, opts)?         // 失败→OS 兜底→剪贴板
if let Some(plan) = annotations::plan_from(&parsed) { deps.overlay.show(&plan)? }
handle.wait() 或 被 cancel
deps.usage.meter(MeterInput { kind: Voice, .. })?      // 事务内与消息一起写
deps.store.commit_turn(...)?
state = Idle; overlay.clear()?; Ok(outcome)
```

取消语义：`cancel()` 置 `AtomicBool` + 中止 speech handle + 丢弃后续 delta；`Thinking` 阶段取消 → 不计用量（H3 的 10 秒规则在任务里，语音按"未产出不计"）。

#### 4.6 `dictation.rs`

```rust
pub struct DictationEngine { /* deps, buffer, stable_prefix, last_partial */ }

pub struct DictationResult { pub final_text: String, pub injected: InjectOutcome, pub learned: Vec<DictEntry> }

impl DictationEngine {
    pub fn start(&self) -> Result<()>;
    pub fn tick(&self) -> Result<Option<String>>;   // 每次 700ms 由外部 tick 驱动；返回本 ticks 新注入的后缀
    pub fn finish(&self) -> Result<DictationResult>;
    pub fn cancel(&self) -> Result<()>;
}

/// 纯函数
pub fn merge_partial(prev: &str, next: &str) -> (String /*common*/, String /*suffix*/);
pub fn cleanup(text: &str, opts: &CleanupOpts) -> String;
pub fn terminal_safe(text: &str) -> String;
pub fn apply_dictionary(text: &str, dict: &[DictEntry]) -> String;
pub fn looks_secret(text: &str) -> bool;
```

`merge_partial` 算法：

```
让 k = 最长公共前缀长度（按 char 边界）
common = next[..k]
suffix = next[k..]
若 prev 不以 common 开头（即 partial 被纠正到更早位置）→ 返回 (prev, "") 并标记 unstable
```

#### 4.7 `memory.rs`

```rust
pub struct Memory { root: PathBuf, profile: Mutex<Vec<String>>, projects: Mutex<Vec<String>> }

pub const PROFILE_MAX_CHARS: usize = 1200;
pub const PROJECTS_MAX_CHARS: usize = 2000;

impl Memory {
    pub fn load(root: &Path) -> Result<Self>;
    pub fn add_preference(&self, line: &str) -> Result<()>;
    pub fn add_project_fact(&self, line: &str) -> Result<()>;
    pub fn forget(&self, needle: &str) -> Result<usize>;
    pub fn profile_text(&self) -> String;     // 不截断（N4）
    pub fn projects_text(&self) -> String;    // 必要时压缩最旧
    pub fn compact_projects(&self) -> Result<()>;
}
```

#### 4.8 `usage.rs`

```rust
pub enum UsageKind { Voice, Dictation, AgentStep, AgentTask }
pub struct MeterInput { pub kind: UsageKind, pub task_id: Option<String>, pub started_at: DateTime<Utc> }

pub struct BudgetState { pub daily_used: f32, pub daily_limit: f32, pub monthly_used: f32, pub monthly_limit: f32, pub percent: f32, pub blocked: bool }

impl UsageMeter {
    pub fn meter(&self, input: MeterInput) -> Result<f32>;   // 返回本次计费单位
    pub fn state(&self, now: DateTime<Utc>) -> Result<BudgetState>;
    pub fn can_start(&self, kind: UsageKind, now: DateTime<Utc>) -> Result<bool>;
    pub fn grant_temporary(&self, now: DateTime<Utc>, pct: f32) -> Result<()>;
}
```

规则表（`tests/usage_rules.rs` 逐条断言）：

| 场景 | 计费 |
|---|---|
| 语音问答完成 | 1.0 |
| 语音开始后 3 秒取消（无输出） | 0 |
| 听写一次（含多次重转写） | 0.5 |
| 任务启动 8 秒内 Stop | 0，且写 `usage.waived` |
| 任务运行 > 20 分钟 | 累计只计 1 次任务费（`AgentTask`） |
| 权限询问/审批交互 | 0 |
| 引导示例 | 0 |
| 中途提额 | 当日 limit += limit*pct |

#### 4.9 `routines.rs`

```rust
pub enum Schedule {
    Interval { minutes: u32 },
    Weekly { days: Vec<Weekday>, hour: u8, minute: u8 },
    Daily { hour: u8, minute: u8 },
}
pub fn parse(s: &str) -> Result<Schedule>;   // "every 30 minutes" | "every monday,thursday at 17:00" | "daily at 09:00"
pub fn next_run(s: &Schedule, after: DateTime<Local>) -> DateTime<Local>;

pub struct Routine { pub id, pub name, pub schedule, pub enabled, pub next_run_at, pub run_count,
                     pub consecutive_failures, pub last_result: Option<String>, pub task_template: String }

pub fn decide(r: &Routine, now, online: bool, awake: bool, budget_ok: bool) -> RoutineDecision;
// Skip | RunNow | Wait(reason) | Pause(reason)
```

规则：连续 3 次失败（非离线原因）→ `Pause`；离线/未醒 → `Wait` 且失败不计；睡眠错过只补一次（`catch_up_once`）。

#### 4.10 `suggest.rs`

```rust
pub struct SuggestBudget { pub ignored_mornings: u8, pub paused_until: Option<Date>, pub ignored_chips: u8, pub chips_paused_until: Option<DateTime<Utc>> }
pub enum SuggestDecision { Show, Suppress(reason) }
pub fn decide_morning(b: &SuggestBudget, now_local, in_call: bool, focus_mode: bool) -> SuggestDecision;
pub fn on_morning_ignored(b: &mut SuggestBudget);
pub fn decide_chips(b: &SuggestBudget, now) -> SuggestDecision;
pub fn on_chip_ignored(b: &mut SuggestBudget, now);
```

阈值：早上 6 点前不出现；连续 3 个早上未打开 → 暂停 1 天，之后每轮翻倍，最长 7 天；chip 连续 8 次未点 → 暂停 1 小时。

#### 4.11 `walkthrough.rs`

```rust
pub struct Step { pub n: u8, pub label: String, pub target: Rect /*global 逻辑坐标*/ }
pub struct Walkthrough { pub steps: Vec<Step>, pub current: usize, pub done: Vec<u8> }
pub enum Advance { Next(Step), Completed, Released, OutOfSteps }

impl Walkthrough {
    pub fn new(steps: Vec<Step>) -> Result<Self>;   // 上限 15
    pub fn on_click(&mut self, p: Point) -> Advance; // 命中 target(+8px 容差) → 下一步；否则 Released
    pub fn on_mouse_move(&mut self, p: Point) -> bool; // 点偏后释放
}
```

#### 4.12 `agent/` 与 `tools/`

```rust
pub enum TaskState { Queued, Running, WaitingUser, WaitingApproval, Completed, CompletedUnverified, Failed, Paused, Canceled }

pub struct Task { pub id, pub name, pub goal, pub state, pub checkpoint: Checkpoint,
                  pub created_at, pub updated_at, pub units: f32, pub workspace: PathBuf, pub steps_done: u32 }

pub struct Checkpoint { pub step: u32, pub goal_notes: String, pub facts: Vec<String>, pub pending_question: Option<String> }

pub struct StepOutcome { pub state: TaskState, pub tool: Option<String>, pub message: String }

impl TaskRunner {
    pub fn create(&self, goal: &str) -> Result<Task>;
    pub fn step(&self, task_id: &str) -> Result<StepOutcome>;         // 单步（测试驱动）
    pub fn run(&self, task_id: &str, cancel: &CancelToken) -> Result<Task>;
    pub fn cancel(&self, task_id: &str) -> Result<()>;
    pub fn resume(&self, task_id: &str) -> Result<Task>;
    pub fn recover_interrupted(&self) -> Result<usize>;
}

pub enum Risk { Read, Write, Destructive, External }
pub struct ToolCall { pub name: String, pub args: serde_json::Value }
pub enum ToolOutcome { Ok { summary: String, data: serde_json::Value }, Denied { reason: String }, Failed { code: String, message: String } }

pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn risk(&self, args: &serde_json::Value) -> Risk;
    fn spec(&self) -> ToolSpec;
    fn run(&self, ctx: &ToolCtx, args: &serde_json::Value) -> Result<ToolOutcome>;
}

pub struct ToolCtx { pub workspace: PathBuf, pub task_id: String, pub http: Arc<dyn HttpTransport>, pub clock: Arc<dyn Clock> }

pub struct Policy;
impl Policy {
    pub fn decide(&self, risk: Risk, approvals: &Approvals, now: DateTime<Utc>) -> Approval;
}
pub struct Approvals { pub per_conversation: bool, pub denied_until: Option<DateTime<Utc>> }
```

工具清单与风险：

| 工具 | 风险 | 参数 | 说明 |
|---|---|---|---|
| `fetch_url` | External | `{url}` | 仅 http/https；SSRF 守卫；HTML→文本；≤ 200k 字符 |
| `http_get_json` | External | `{url, headers?}` | JSON 响应 ≤ 1MB |
| `fs_list` | Read | `{path}` | 工作区内 |
| `fs_read` | Read | `{path, max_bytes?}` | ≤ 256KB |
| `fs_write` | Write | `{path, content, mode?}` | 工作区内；原子写 + 冲突副本 |
| `fs_delete` | Destructive | `{path}` | 工作区内；先备份到 `.trash/` |
| `shell` | Destructive | `{command, cwd?}` | 默认关闭（`config.tools.shell=false`） |
| `ask_user` | Read | `{question, options?}` | 挂起任务 |
| `finish` | Read | `{result, artifacts?}` | 结束并给证据 |

**agent 循环**（`run`）：见 §09 流程 3。

#### 4.13 `store/`

DDL 见 §05。`Store` 为具体类型（非 trait），生产用文件，测试用 `:memory:`。

```rust
pub struct Store { conn: Mutex<Connection> }
impl Store {
    pub fn open(path: &Path) -> Result<Self>;
    pub fn open_memory() -> Result<Self>;
    pub fn migrate(&self) -> Result<()>;
    pub fn insert_session(&self) -> Result<String>;
    pub fn append_message(&self, m: &MessageRow) -> Result<()>;
    pub fn commit_turn(&self, turn: &Turn) -> Result<()>;     // 事务：message + usage + task state
    pub fn recent_messages(&self, session_id: &str, limit: usize) -> Result<Vec<MessageRow>>;
    pub fn upsert_task(&self, t: &Task) -> Result<()>;
    pub fn task(&self, id: &str) -> Result<Option<Task>>;
    pub fn list_tasks(&self, state: Option<TaskState>) -> Result<Vec<Task>>;
    pub fn record_step(&self, task_id: &str, s: &StepRecord) -> Result<()>;
    pub fn audit(&self, a: &AuditRow) -> Result<()>;
    pub fn usage_between(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<UsageRow>>;
    pub fn delete_everything(&self) -> Result<DeletionReport>;
}
```

### 05 数据模型与 DDL

```sql
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
PRAGMA user_version=1;

CREATE TABLE sessions (
  id TEXT PRIMARY KEY, created_at TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'ask'
);
CREATE TABLE messages (
  id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  role TEXT NOT NULL, content TEXT NOT NULL, model TEXT, created_at TEXT NOT NULL,
  meta_json TEXT
);
CREATE INDEX idx_messages_session ON messages(session_id, created_at);

CREATE TABLE tasks (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, goal TEXT NOT NULL, state TEXT NOT NULL,
  checkpoint_json TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  heartbeat_at TEXT, units REAL NOT NULL DEFAULT 0, steps_done INTEGER NOT NULL DEFAULT 0,
  parent_session_id TEXT, result TEXT, error_code TEXT
);
CREATE INDEX idx_tasks_state ON tasks(state, updated_at);

CREATE TABLE task_steps (
  id INTEGER PRIMARY KEY AUTOINCREMENT, task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  step INTEGER NOT NULL, kind TEXT NOT NULL, tool TEXT, request_json TEXT, result_json TEXT,
  ok INTEGER NOT NULL, code TEXT, created_at TEXT NOT NULL
);

CREATE TABLE usage_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, units REAL NOT NULL,
  task_id TEXT, session_id TEXT, waived INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL,
  meta_json TEXT
);
CREATE INDEX idx_usage_created ON usage_events(created_at);

CREATE TABLE audit_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT, task_id TEXT, tool TEXT NOT NULL, risk TEXT NOT NULL,
  args_digest TEXT NOT NULL, outcome TEXT NOT NULL, created_at TEXT NOT NULL
);

CREATE TABLE metrics_daily (
  day TEXT NOT NULL, name TEXT NOT NULL, value REAL NOT NULL, PRIMARY KEY(day, name)
);

CREATE TABLE routines (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, schedule TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1,
  next_run_at TEXT, run_count INTEGER NOT NULL DEFAULT 0, failures INTEGER NOT NULL DEFAULT 0,
  last_result TEXT, task_template TEXT NOT NULL
);

CREATE TABLE dict_entries (
  heard TEXT PRIMARY KEY, corrected TEXT NOT NULL, learned_at TEXT NOT NULL
);
```

数据字典（关键字段语义）：

| 字段 | 语义 | 约束 |
|---|---|---|
| `tasks.state` | 状态机当前值 | 只能由 `agent::transition()` 写，且必须满足 §11 表 |
| `tasks.checkpoint_json` | 恢复所需最小事实 | 每步后覆盖写，含 `step`/`goal_notes`/`facts` |
| `usage_events.units` | 本次计费单位 | 由 `usage::rules` 计算，禁止调用方传入 |
| `usage_events.waived` | 是否豁免（10 秒规则） | 1 时不进入预算求和 |
| `audit_log.args_digest` | 参数 SHA-256 前 16 hex | 不存明文参数（防密钥泄漏） |

### 06 存储、事务、读写路径与一致性

| 写入路径 | 事务边界 |
|---|---|
| `commit_turn` | messages + usage_events + tasks.state 同一事务 |
| `record_step` | task_steps + tasks.checkpoint/heartbeat 同一事务 |
| 迁移 | 单事务（DDL + `user_version`） |
| 删除 | 单事务删表 → 事务提交后再删文件（文件失败记 `DeletionReport`） |

无事件总线。断点续做依赖 `checkpoint_json`（权威）。

### 07 同步接口与调用契约

见 §04 的 trait 签名。补充**调用约定**：

- `ScreenSource::capture_all` 必须由 `CaptureGate` 触发；未开 gate 调用 → 测试断言为 0 次。
- `ChatProvider::complete_stream` 必须每收到一个 delta 立即回调；不得攒批。
- `TextInjector::inject` 必须在 2s 内返回；超时视为 `PasteFailed`。
- 所有 `Result` 使用 `error::Error`；跨界错误必须携带 `code()`（见 §13）。

### 08 事件与异步协议

无跨进程事件。进程内"事件"以 `AppEvent` 枚举经 `dyn Fn(AppEvent)` 通知壳层：

```rust
pub enum AppEvent {
    State(State), Transcript { text: String, partial: bool },
    Delta { text: String }, Answer { text: String }, Annotations { plan_id: String },
    TaskUpdated { id: String, state: TaskState, message: String },
    ApprovalRequest { id: String, tool: String, risk: Risk },
    Usage { percent: f32 }, Notice { code: String, message: String },
    Dictation { text: String, partial: bool },
}
```

顺序保证：同一会话内事件按产生顺序投递（单线程编排器）。

---

## C. 执行逻辑与动态行为

### 09 关键场景端到端设计

**流程 1：ASK**

```
hotkey_down → gate.open(Hotkey) → audio.start() → state=Listening
          → emit(State::Listening)
hotkey_up   → audio.stop() → stt → capture_all (并行) → prompt.build
          → chat.complete_stream → emit(Delta)*
          → annotations.parse → emit(Answer) → emit(Annotations)
          → speech.speak (失败→OsfFallback→剪贴板) → state=Speaking
          → wait/被取消 → usage.meter → store.commit_turn → emit(State::Idle) → overlay.clear()
```

**流程 2：任务**

```
create(goal) → Task{Queued} → 调度线程 step()
step: context → chat.complete(tools) →
  if tool_calls:
     for call: policy.decide → execute → record_step → checkpoint
     state=Running
  if finish: 证据核验 → Completed | CompletedUnverified
  if ask_user: WaitingUser
  if 步数>30: Failed(too_many_steps)
```

**流程 3：听写**

```
hotkey_down(FnCtrl) → audio.start → state=Dictating
tick(700ms): clip(buffer) → stt → merge_partial → inject(suffix) → emit(Dictation)
hotkey_up → final transcribe → diff(末 20s) → 需要则改正 → cleanup（可关）→ 词典学习 → 计费
```

### 10 状态机与转换规则

见 tech-prd §07 表。补充：`Dictating` 与 `Listening` 互斥；`Driving` 仅由任务产生，用户任何输入立即退出。

### 11 任务状态机（权威表）

| 当前 | 事件 | 新状态 | 守卫 |
|---|---|---|---|
| Queued | 调度 | Running | 有预算 & 并发<上限 |
| Queued | 取消 | Canceled | — |
| Running | 工具成功 | Running | — |
| Running | 需要批准 | WaitingApproval | risk ∈ {Write(区外), Destructive, External(上传)} |
| WaitingApproval | 用户批准 | Running | — |
| WaitingApproval | 用户拒绝 | Running（改道）或 Failed | 模型重试上限 |
| Running | ask_user | WaitingUser | — |
| WaitingUser | 用户回复 | Running | — |
| Running | finish + 证据 | Completed | artifact/HTTP 结果存在 |
| Running | finish 无证据 | CompletedUnverified | — |
| Running | 步数>30 / 预算耗尽 / 20min 心跳超时 | Failed / Paused | — |
| Running | 取消 | Canceled | — |
| 任意非终态 | 应用退出 | Paused(interrupted) | 心跳 > 20min 时在启动时判定 |
| Paused | retry | Running | 检查点存在 |
| Failed | retry | Running | 重试次数 < 3 |

### 12 并发、调度、取消与资源协调

| 机制 | 实现 |
|---|---|
| 语音单飞 | `Mutex<Option<Session>>` + `AtomicBool cancel` |
| 任务并发 | `Semaphore`（默认 3）；队列 FIFO 上限 20 |
| 取消 | `CancelToken { AtomicBool }`，每个工具调用点检查 |
| 定时器 | 单 `Scheduler` 线程，每 30s tick；不用 `sleep` 精确计时 |
| SQLite | 单写 `Mutex<Connection>` + WAL |
| 截图 | 每显示器一线程（`rayon`/手工线程），合并后有 1.5s 硬超时 |
| 听写 tick | 由调用方定时驱动（壳层 `setInterval`），core 不含定时器 |
| 心跳 | 每次 `record_step` 更新 `heartbeat_at` |

### 13 错误分类、重试、恢复、补偿

```rust
pub enum Error {
    // 用户可修复
    NoApiKey, AuthFailed, RegionBlocked, NoPermission(Permission), NoModel,
    // 暂时性
    RateLimited { retry_after: Option<Secs> }, Outage, Timeout, Network,
    // 输入
    TooShort, EmptyAudio, BadRequest(String), BudgetExhausted, PolicyDenied(String),
    // 内部
    State(StateError), Store(String), Protocol(String), Io(String), Internal(String),
}
impl Error { pub fn code(&self) -> &'static str; pub fn user_message(&self) -> String; pub fn retryable(&self) -> bool; }
```

| code | 用户文案 | 可重试 |
|---|---|---|
| `E_NO_KEY` | 还没有配置 API key。打开设置 → 模型 | 否 |
| `E_AUTH` | 密钥被拒绝，请在设置里更新 | 否 |
| `E_REGION` | 上游按地区拦截了请求。换一个 provider，或改用本地兼容端点 | 否 |
| `E_RATE` | 请求太频繁，稍后自动重试 | 是（2 次） |
| `E_OUTAGE` | 模型服务暂时不可用 | 是 |
| `E_TIMEOUT` | 超时了 | 是 |
| `E_PERM` | 缺少「屏幕录制」权限 | 否 |
| `E_BUDGET` | 今天/本月预算用完了 | 否 |
| `E_POLICY` | 这个操作需要你确认 | 否 |
| `E_STATE` | 内部状态异常（已记录） | 否 |
| `E_PROTOCOL` | 模型返回了无法解析的内容 | 否（降级纯文本） |

### 14 前端、交互实现与多端状态

| 窗口 | 尺寸 | 行为 |
|---|---|---|
| `overlay` | 覆盖所有显示器 | `transparent`, `always_on_top`, `skip_taskbar`, `ignore_cursor_events(true)`，每屏一个窗口（Tauri 多 window） |
| `notch` | 顶部长条 420×36，展开 420×520 | 无边框、圆角、`always_on_top`；Linux/Windows 为悬浮岛（top: 8px） |
| `main` | 900×640 | 设置/聊天/引导；常规窗口 |

渲染：Canvas 2D；手绘风格 = 二次贝塞尔 + 每段随机扰动（seed = plan_id 哈希，保证重绘一致）。

状态同步：前端只调命令 + 监听 `AppEvent`；不缓存权威状态。

### 15 安全控制、授权执行与隐私设计

见 tech-prd §12；实现点：

| 控制 | 代码位置 | 测试 |
|---|---|---|
| CaptureGate | `privacy.rs` | `capture_gate.rs` |
| 脱敏 | `security::redact` | `redact.rs` |
| 沙箱 | `tools::fs::resolve_in_workspace` | `tools_fs.rs` |
| SSRF | `tools::net::guard_url` | `tools_net.rs` |
| 注入守卫 | `dictation::guards` | `inject_guards.rs` |
| 删除 | `store::delete_everything` + `deletion` | `deletion.rs` |

### 16 第三方系统、插件与复用组件

| 依赖 | 用途 | 失败降级 |
|---|---|---|
| `xcap` | 截图 | 报 `NoPermission`/`NoDisplay` |
| `enigo` | 注入 | 剪贴板路径 |
| `cpal` | 录音 | 报错并提示选设备 |
| `rubato`?（不用）| — | 自己写线性重采样 |
| `keyring` | 密钥 | 加密文件 |
| OS TTS | 兜底 | 静默 + 字幕 |

---

## E. 运行、资源与演进

### 17 性能预算与资源控制（落地参数）

| 参数 | 值 |
|---|---|
| 截图长边 | 1280（模型）/ 原始（绘制锚点） |
| JPEG 质量 | 0.8 |
| 历史消息注入 | 最近 10 轮（ask）/ 20 条（task） |
| 记忆注入 | profile ≤ 1200 字（不截断，超限只警告）；projects ≤ 2000 字（压缩） |
| SSE 读取缓冲 | 8KB |
| 工具输出截断 | 8k token 估算（≈32k 字符） |
| 并发上限 | 任务 3，模型请求 4，截图 = 显示器数 |

### 18 日志、指标、追踪与审计

事件名（固定字符串，便于 grep）：

`app.start`, `app.exit`, `hotkey.down`, `hotkey.up`, `capture.gate`, `capture.done`,
`voice.first_delta`, `voice.first_audio`, `voice.done`, `voice.cancel`,
`annotation.parsed`, `annotation.dropped`, `annotation.fallback_screen`,
`provider.request`, `provider.retry`, `provider.error`,
`task.created`, `task.step`, `task.state`, `task.cancel`, `task.recover`,
`tool.call`, `tool.denied`, `tool.failed`, `policy.approval`,
`usage.meter`, `usage.waived`, `budget.block`, `memory.write`, `memory.compact`,
`dictation.tick`, `dictation.done`, `inject.fallback`, `secret.fallback`, `deletion.step`.

### 19 配置、部署与运行生命周期

`config.toml`（完整 schema）：

```toml
config_version = 1

[provider]
kind = "openai"            # openai | anthropic | openai_compatible
base_url = "https://api.openai.com/v1"
chat_model = ""            # 空 = 首次引导时选择
fast_model = ""            # 空 = 同 chat_model
deep_model = ""
stt_model = "whisper-1"
tts_model = "gpt-4o-mini-tts"
tts_voice = "alloy"
stt_base_url = ""          # 空 = 同 base_url
region_hint = ""

[voice]
captions = true
speech_speed = 1.0
max_answer_tokens = 700
language = ""              # 空 = 自动

[shortcuts]
talk = "control+alt"
dictation = "fn+control"

[budget]
daily_units = 200.0
monthly_units = 3000.0
weights = { voice = 1.0, dictation = 0.5, agent_step = 3.0 }

[tools]
shell = false
allow_private_network = false
workspace = ""             # 空 = <data>/workspace
max_steps = 30

[privacy]
debug_allow_text = false
keep_screenshots_hours = 24

[ui]
cursor_color = "#ffd166"
hide_cursor_when_idle = false
interface_language = "zh"
```

生命周期：`app.start` → 迁移 → 单实例锁 → 权限检查 → 托盘 + 热键 → 调度线程 → 事件循环；退出：保存检查点、停止调度、释放锁。

### 20 迁移、版本兼容与回退

| 变更类型 | 策略 |
|---|---|
| 新增表/列 | `ALTER TABLE` 可空或带默认值 |
| 重命名 | 新增 + 双写 + 后续迁移删除 |
| 删除列 | 先停写，下一版本删 |
| 记忆格式 | 按行解析，未知行保留原样 |
| 配置字段 | 未知字段保留（`#[serde(flatten)] extra`） |
| 注解语法 | 只增不改；旧标签永久可解析 |
| 回退 | 迁移前 `app.db.bak`；旧版本遇到更高 `user_version` → 只读模式 + 提示 |

### 21 可测试性设计与验证接口

| 设计 | 目的 |
|---|---|
| 所有外部世界都是 trait | 假实现替换 |
| `Clock` 注入 | 不 `sleep`，可直接推进时间 |
| 纯函数化核心算法（`parse_reply`/`merge_partial`/`route`/`policy`/`next_run`） | 表驱动单测 |
| `Deps` 集中装配 | 测试构造 5 行 |
| `AppEvent` 回调 | 断言事件序列 |
| `TaskRunner::step` | 单步断言，不依赖线程 |

### 22 设计评审、未决问题与变更记录

| 未决问题 | 影响 | 计划 |
|---|---|---|
| `xcap` 在 macOS 26 的 Retina 缩放是否与 `scale` 一致 | 指的位置可能偏 | 本地冒烟时用十字线校准（`selftest --calibrate`） |
| `rdev` 在 Linux Wayland 下不可用 | 带你做不可用 | 检测并禁用，文档说明 |
| `keyring` 无 secret-service | 降级 | 已实现降级 |

变更记录：v1.0 初版（2026-11-23）。
