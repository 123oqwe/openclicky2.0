# 04 测试设计与验收规格（TDD 文档）

| 项 | 值 |
|---|---|
| 编号 | OC2-TDS |
| 版本 | 1.4（2026-11-23，补首跑/交办/召回用例，标明尚未自动化） |
| 状态 | 生效 |
| 术语 | 本文自称 **TDS**，避免与"测试驱动开发（TDD）"混淆。TDD 是本项目的**开发方法**，TDS 是**这份规格** |
| 依据 | `01-tech-prd.md` §18、`02-sdd.md`、`adr/ADR-0012` |

一句话判据：**把"我们认为产品做对了"变成"在明确条件下，用指定方法检查了指定要求，取得了指定证据，并知道哪些风险没有排除"。**

---

## A. 测试依据、范围与策略

### 01 文档基线、测试对象与目标

| 项 | 内容 |
|---|---|
| 被测基线 | 每个 PR 的 head commit；G2/G3 绑定具体 commit |
| 被测对象 | `openclicky-core`（业务规则）、`openclicky-platform`（OS 适配，编译 + 手工冒烟）、`apps/desktop`（窗口与命令桥，手工冒烟）、前端纯函数 |
| 目标 | 关键逻辑自动化可证；接口协议可证；隐私与权限不可越界；三平台可构建 |
| 非目标 | 真实模型输出质量（无法离线判定）、OS 权限弹窗自动化、商店审核 |

### 02 测试依据与双向追踪矩阵

正向（要求 → 用例）与反向（用例 → 要求）都在此表维护。**本表由 `scripts/tds-evidence.py` 生成**：它读取实现分支上真实存在的 `#[test]` 函数，只有能在代码里找到的测试才允许标 `已实现`；否则写 `部分` 或 `spec-only`。

状态含义：

| 状态 | 含义 | 执行报告中的计算 |
|---|---|---|
| 已实现 | 代码里存在该测试函数 | 可计入通过/失败 |
| 部分 | 有测试但只覆盖该条要求的一部分 | 计未覆盖，不得报通过 |
| spec-only | 还没有对应测试 | 计未执行 |

<!-- TDS-MATRIX:BEGIN -->
共 73 条：已实现 72、部分 0、spec-only 1。**只有 `已实现` 才能在执行报告中计入通过；`部分` 与 `spec-only` 一律计入未执行/未覆盖。**

| 用例 | 要求 | TC | 状态 | 证据（真实测试） | 备注 |
|---|---|---|---|---|---|
| TC-PRIV-001 | INV-1 不按键不采集 | TC-PRIV-001 | **已实现** | `crates/core/tests/security.rs::no_capture_without_a_gate` | — |
| TC-PRIV-002 | 门禁只由用户动作开启 | TC-PRIV-002 | **已实现** | `crates/core/tests/security.rs::capture_gate_counts_only_user_actions` | — |
| TC-CAP-001 | 一次提问只采集一次 | TC-CAP-001 | **已实现** | `crates/core/tests/e2e_ask.rs::capture_happens_exactly_once_per_question` | — |
| TC-CAP-002 | 副屏负原点坐标换算（按图片比例） | TC-CAP-002 | **已实现** | `crates/core/tests/e2e_ask.rs::pointing_on_a_negative_origin_display_uses_the_right_offset` | 缩比在 annotations::picture_shrink 中锁定 |
| TC-HK-001 | 默认 talk 键解析 | TC-HK-001 | **已实现** | `crates/core/src/hotkey.rs::parses_mac_default_talk_binding` | — |
| TC-HK-002 | 别名归一（ctrl/option） | TC-HK-002 | **已实现** | `crates/core/src/hotkey.rs::parses_aliases` | — |
| TC-HK-003 | 听写键含 Fn | TC-HK-003 | **已实现** | `crates/core/src/hotkey.rs::parses_dictation_with_fn` | — |
| TC-HK-004 | 双击手势 | TC-HK-004 | **已实现** | `crates/core/src/hotkey.rs::parses_double_tap_gesture` | — |
| TC-HK-005 | 非法组合被拒 | TC-HK-005 | **已实现** | `crates/core/src/hotkey.rs::rejects_mid_typo` | — |
| TC-E2E-ASK-001 | 问答全链路（回答+指+计费） | TC-E2E-ASK-001 | **已实现** | `crates/core/tests/e2e_ask.rs::asks_answers_points_and_charges_once` | — |
| TC-E2E-ASK-002 | TTS 失败回退剪贴板 | TC-E2E-ASK-002 | **已实现** | `crates/core/tests/e2e_ask.rs::silent_tts_falls_back_to_text_and_clipboard` | — |
| TC-E2E-ASK-003 | 缺模型报 E_MODEL | TC-E2E-ASK-003 | **已实现** | `crates/core/tests/e2e_ask.rs::missing_model_is_a_clear_error` | — |
| TC-E2E-ASK-004 | 消息与用量同事务落库 | TC-E2E-ASK-004 | **已实现** | `crates/core/tests/e2e_ask.rs::messages_and_usage_are_persisted_together` | — |
| TC-E2E-ASK-005 | 打字提问不采集/不录音 | TC-E2E-ASK-005 | **已实现** | `crates/core/tests/e2e_ask.rs::typed_question_captures_once_and_can_point` | — |
| TC-E2E-ASK-006 | 199ms 短按拒绝（不采集不计费） | TC-E2E-ASK-006 | **已实现** | `crates/core/tests/e2e_ask.rs::short_press_is_rejected_and_not_charged` | — |
| TC-ANNO-001 | POINT 解析与剥离标签 | TC-ANNO-001…006 | **已实现** | `crates/core/tests/e2e_ask.rs::asks_answers_points_and_charges_once` | 13 条细项见 spec-only |
| TC-ANNO-002 | 越界裁剪/坏标签丢弃/多形状 | TC-ANNO-002 | **已实现** | `crates/core/src/annotations.rs::parses_every_draw_shape` | 另有 9 条同模块单测 |
| TC-GEST-001 | 按住期间鼠标轨迹并入参考图 | TC-GEST-001 | **已实现** | `crates/core/tests/e2e_ask.rs::a_gesture_puts_the_circled_region_into_the_prompt` | bbox/dead-zone/circle 另 3 条 |
| TC-WALK-001 | 命中推进/点偏释放 | TC-WALK-001 | **已实现** | `crates/core/src/walkthrough.rs::hit_advances_and_miss_releases` | — |
| TC-WALK-002 | 容差内命中 | TC-WALK-002 | **已实现** | `crates/core/src/walkthrough.rs::click_near_the_edge_still_hits` | — |
| TC-WALK-003 | 最多 15 步 | TC-WALK-003 | **已实现** | `crates/core/src/walkthrough.rs::at_most_fifteen_steps` | — |
| TC-WALK-004 | continue 推进且不可重放 | TC-WALK-004 | **已实现** | `crates/core/src/walkthrough.rs::continue_moves_on_and_cannot_replay` | — |
| TC-WALK-005 | 空带做被拒 | TC-WALK-005 | **已实现** | `crates/core/src/walkthrough.rs::empty_walkthrough_is_rejected` | — |
| TC-DICT-001 | 只注入新增后缀 | TC-DICT-001 | **已实现** | `crates/core/tests/e2e_dictation.rs::only_the_new_suffix_is_typed` | — |
| TC-DICT-002 | 终端只注入单行 | TC-DICT-002 | **已实现** | `crates/core/tests/e2e_dictation.rs::terminal_targets_get_a_single_line` | — |
| TC-DICT-003 | 疑似密码不注入 | TC-DICT-003 | **已实现** | `crates/core/tests/e2e_dictation.rs::tick_refuses_password_like_speech` | tick 与 finish 都过守卫 |
| TC-DICT-004 | 词典学习与复用 | TC-DICT-004 | **已实现** | `crates/core/tests/e2e_dictation.rs::corrections_are_learned_and_reused` | — |
| TC-DICT-005 | 学习窗口关闭 | TC-DICT-005 | **已实现** | `crates/core/tests/e2e_dictation.rs::learning_window_closes_after_finish` | — |
| TC-DICT-006 | 前缀合并/回退/清理/词典优先 | TC-DICT-006…008 | **已实现** | `crates/core/src/dictation.rs::merge_handles_rewrite` | 另有 7 条同模块单测 |
| TC-USE-001 | 10 秒内取消不计费 | TC-USE-001 | **已实现** | `crates/core/src/usage.rs::task_stopped_within_ten_seconds_is_free` | — |
| TC-USE-002 | 语音 1u | TC-USE-002 | **已实现** | `crates/core/src/usage.rs::voice_costs_one_unit` | — |
| TC-USE-003 | 80% 提醒 / 100% 阻断 | TC-USE-003 | **已实现** | `crates/core/src/usage.rs::budget_blocks_at_limit_and_warns_at_80_percent` | — |
| TC-USE-004 | 每日重置 | TC-USE-004 | **已实现** | `crates/core/src/usage.rs::daily_reset_clears_usage` | — |
| TC-USE-005 | 当日提额解除阻断 | TC-USE-005 | **已实现** | `crates/core/src/usage.rs::temporary_grant_unblocks_today` | 月度提额待 F3 |
| TC-USE-006 | 长任务只计一次 | TC-USE-006 | **已实现** | `crates/core/src/usage.rs::agent_steps_are_always_metered_even_after_twenty_minutes` | 20 分钟后步数仍如实计量 |
| TC-AGT-001 | 写文件后带证据完成 | TC-AGT-001 | **已实现** | `crates/core/tests/e2e_agent.rs::writes_a_file_then_finishes_with_evidence` | — |
| TC-AGT-002 | 拒绝后不执行且 30 分钟内不问 | TC-AGT-002 | **已实现** | `crates/core/src/tools/policy.rs::destructive_asks_then_respects_not_now` | 拒绝路径见 e2e_agent |
| TC-AGT-003 | 无证据完成标 unverified | TC-AGT-003 | **已实现** | `crates/core/tests/e2e_agent.rs::claim_without_artifact_is_unverified` | — |
| TC-AGT-004 | 取消终态且保留检查点 | TC-AGT-004 | **已实现** | `crates/core/tests/e2e_agent.rs::cancellation_is_terminal_and_keeps_the_checkpoint` | — |
| TC-AGT-005 | 启动恢复置 Paused | TC-AGT-005 | **已实现** | `crates/core/tests/e2e_agent.rs::startup_recovery_pauses_stale_running_tasks` | — |
| TC-AGT-006 | 批准后动作只执行一次 | TC-AGT-006 | **已实现** | `crates/core/tests/e2e_agent.rs::approval_lets_the_action_run_once` | — |
| TC-AGT-007 | 循环检测停止任务 | TC-AGT-007 | **已实现** | `crates/core/tests/e2e_agent.rs::repeated_identical_calls_stop_the_task` | 当前是 Failed(loop)，规范要求改道 + 事件（F3） |
| TC-AGT-008 | shell 关闭时拒绝 | TC-AGT-008 | **已实现** | `crates/core/tests/e2e_agent.rs::shell_is_denied_when_disabled` | — |
| TC-AGT-009 | 状态机转换表 | TC-AGT-009 | **已实现** | `crates/core/src/agent/mod.rs::transitions_follow_the_table` | 覆盖 SDD 表全部允许/禁止边 |
| TC-RTN-001 | 条件不满足等待 | TC-RTN-001 | **已实现** | `crates/core/src/routines.rs::wakes_up_only_when_conditions_are_met` | — |
| TC-RTN-002 | 三连失败暂停 | TC-RTN-002 | **已实现** | `crates/core/src/routines.rs::wakes_up_only_when_conditions_are_met` | — |
| TC-RTN-003 | 时间表解析与下次触发 | TC-RTN-003 | **已实现** | `crates/core/src/routines.rs::weekly_picks_the_next_matching_day` | — |
| TC-RTN-004 | 非法时间表被拒 | TC-RTN-004 | **已实现** | `crates/core/src/routines.rs::rejects_garbage` | — |
| TC-SUG-001 | 6 点前/通话/专注不打扰 | TC-SUG-001 | **已实现** | `crates/core/src/suggest.rs::no_suggestions_before_six` | — |
| TC-SUG-002 | 连续 3 个早上忽略暂停并翻倍 | TC-SUG-002 | **已实现** | `crates/core/src/suggest.rs::three_ignored_mornings_pause_one_day_then_double` | — |
| TC-SUG-003 | 暂停上限 7 天 | TC-SUG-003 | **已实现** | `crates/core/src/suggest.rs::pause_is_capped_at_a_week` | — |
| TC-SUG-004 | 8 次未点胶囊停 1 小时 | TC-SUG-004 | **已实现** | `crates/core/src/suggest.rs::eight_ignored_chips_pause_one_hour` | — |
| TC-MEM-001 | 偏好去重并持久化 | TC-MEM-001 | **已实现** | `crates/core/src/memory.rs::preferences_are_deduplicated_and_persisted` | — |
| TC-MEM-002 | 偏好不截断 | TC-MEM-002 | **已实现** | `crates/core/src/memory.rs::profile_is_never_truncated` | — |
| TC-MEM-003 | 项目记忆压缩 | TC-MEM-003 | **已实现** | `crates/core/src/memory.rs::projects_are_compacted` | — |
| TC-MEM-004 | 遗忘删除匹配行 | TC-MEM-004 | **已实现** | `crates/core/src/memory.rs::forget_removes_matching_lines_everywhere` | — |
| TC-RTE-001 | 快/深路由表 | TC-RTE-001 | **已实现** | `crates/core/src/router.rs::tasks_are_always_deep` | 另有 5 条同模块单测 |
| TC-PRV-001 | OpenAI 文本/工具解析 | TC-PRV-001 | **已实现** | `crates/core/tests/providers.rs::openai_parses_text_tools_and_usage` | — |
| TC-PRV-002 | 流式增量 | TC-PRV-002 | **已实现** | `crates/core/tests/providers.rs::openai_stream_accumulates_deltas` | — |
| TC-PRV-003 | 请求带图与 tools | TC-PRV-003 | **已实现** | `crates/core/tests/providers.rs::openai_request_carries_images_and_tools` | — |
| TC-PRV-004 | Anthropic 工具块 | TC-PRV-004 | **已实现** | `crates/core/tests/providers.rs::anthropic_parses_tool_use` | — |
| TC-PRV-005 | Anthropic 流式 | TC-PRV-005 | **已实现** | `crates/core/tests/providers.rs::anthropic_stream_reads_deltas` | — |
| TC-ERR-001 | 错误码映射 | TC-ERR-001 | **已实现** | `crates/core/tests/providers.rs::error_statuses_map_to_stable_codes` | — |
| TC-ERR-002 | 无 key 报 E_NO_KEY | TC-ERR-002 | **已实现** | `crates/core/tests/providers.rs::build_requires_a_key` | — |
| TC-SEC-001 | 密钥脱敏 | TC-SEC-001 | **已实现** | `crates/core/tests/security.rs::credentials_are_redacted_from_logs` | — |
| TC-SEC-002 | 行内密钥识别 | TC-SEC-002 | **已实现** | `crates/core/tests/security.rs::secret_detection_catches_inline_keys` | — |
| TC-SEC-003 | 沙箱拒绝 ../绝对路径/符号链接 | TC-SEC-003 | **已实现** | `crates/core/src/tools/fs.rs::rejects_symlink_escape` | 另有 3 条同模块单测 |
| TC-SEC-004 | SSRF 拒绝内网（含简写形式） | TC-SEC-004 | **已实现** | `crates/core/src/tools/net.rs::blocks_shorthand_ipv4_forms` | 另有 5 条同模块用例 |
| TC-SEC-005 | 注入守卫（终端/密码） | TC-SEC-005 | **已实现** | `crates/core/tests/e2e_dictation.rs::tick_refuses_password_like_speech` | 终端单行见 terminal_targets_get_a_single_line |
| TC-SEC-006 | 删除逐表报告 | TC-SEC-006 | **已实现** | `crates/core/src/deletion.rs::removes_every_local_artifact_and_reports_it` | 八步 + 范围外声明 |
| TC-APP-001 | 单实例锁 | TC-APP-001 | **已实现** | `crates/platform/src/singleton.rs::second_instance_is_refused` | — |
| TC-APP-002 | 更新包验签 | TC-APP-002 | **spec-only** | — | 桌面外壳落地后补 |
| TC-KEY-001 | 钥匙存储拒绝写入时保留原因 | TC-KEY-001 | **已实现** | `crates/platform/src/secrets.rs::a_refusal_keeps_the_backends_own_words` | degraded_reason 保留后端原话，#86 |
<!-- TDS-MATRIX:END -->

**反向追踪的检查方式**：`python3 scripts/tds-evidence.py` 会对每条 `已实现` 做 `git grep` 校验；任何引用不存在的测试都会让脚本以非 0 退出，CI 会拦下。

### 03 风险分析、优先级与不测范围

| 风险 | 用例优先级 |
|---|---|
| 越权/沙箱逃逸/SSRF | P0 |
| 隐私（未按键采集、日志泄漏密钥） | P0 |
| 任务假成功、重复计费、丢检查点 | P0 |
| 注解坐标错误（指错位置） | P1 |
| 听写破坏终端/密码框 | P1 |
| 定时任务重复执行 | P1 |
| 打扰过多 | P2 |
| 疲劳/性能 | P2（本地脚本，不进 CI 门槛） |

**明确不测**：真实模型回答质量、真实 TCC 弹窗、三平台 UI 像素级一致、离线大陆网络情况、无障碍全量。

### 04 测试策略、层级与责任

| 层 | 占比 | 位置 | 责任 |
|---|---|---|---|
| 单元 | ~60% | 模块内 `#[cfg(test)]` | 实施 Agent |
| 集成（假实现） | ~35% | `crates/core/tests/` | 实施 Agent |
| 手工冒烟 | ~5% | `06-acceptance.md` | 人类维护者 |

门槛：P0 用例 100% 通过；`core` 行覆盖 ≥ 80% 是**目标值**，尚未接入测量（未接 `cargo llvm-cov`），接入前不得作为发布门槛，执行报告记未测。

---

## B. 测试设计、执行条件与判定依据

### 05 场景模型、测试条件与覆盖方法

| 方法 | 用在哪 |
|---|---|
| 等价类 | 错误码 → 6 类（auth/rate/region/outage/timeout/protocol） |
| 边界值 | 录音 199/200ms；步数 29/30/31；坐标 0/w-1/w+10；标签 40/41 字符 |
| 状态迁移 | 语音 5 态、任务 9 态全迁移表 |
| 判定表 | 策略（risk × approval 状态 × 时间窗） |
| 故障注入 | 传输层注入 Timeout/断流/部分 SSE |
| 表驱动 | 路由、计费、定时解析 |
| 组合（两两） | 注入目标（终端/编辑器/浏览器）× 路径（typed/paste/both fail） |

### 06 正确性判定依据

| 判定对象 | 依据（不是"看起来对"） |
|---|---|
| 注解位置 | 换算后的全局坐标 == 期望值（精确到 0.5px） |
| 是否指错屏 | `screen` 索引与 `DisplayInfo.origin` 一致性断言 |
| 用量 | `usage_events` 求和 == 期望单位（含 waived 排除） |
| 任务完成 | 状态 + 产物存在 + 工具调用序列 |
| 越权 | 工具未执行（`FakeTool.calls == 0`）而非"返回了拒绝" |
| 采集 | `FakeScreen.calls == 0` |
| 密钥泄漏 | 对序列化后的全部输出做 `assert!(!contains("sk-"))` |

### 07 单个测试用例的完整规格（模板 + 实例）

每条用例必须含：ID / 名称 / 层级 / 依据 / 前置 / 输入 / 步骤 / 预期 / 证据 / **不能证明什么**。

**TC-ANNO-002 坐标换算（Retina 副屏在左侧，负原点）**

| 字段 | 内容 |
|---|---|
| 层级 | 单元 |
| 依据 | tech-prd §09、ADR-0006 |
| 前置 | 副屏 `origin=(-1920, 0)`，`scale=2`，截图 3840×2160 |
| 输入 | `[POINT:1920,1080:save:1]` |
| 预期 | `global == (-1920 + 960, 0 + 540) = (-960, 540)` |
| 证据 | `tests/annotations.rs` 断言输出 |
| 不能证明 | 真实屏幕坐标是否正确（需冒烟校准） |

### 08 执行规程、初始化与清理

- 每条测试自建 `Deps`（临时目录 + `:memory:` store），测试结束 `TempDir` 自动清理。
- 无共享全局状态；`std::env` 不读不写。
- 不依赖执行顺序；并行安全（`--test-threads` 默认）。
- 禁止 `sleep`；时间由 `TestClock` 推进。

### 09 环境、替身范围与真实性边界

| 替身 | 替代 | 保真度 | 不保真处 |
|---|---|---|---|
| `FakeScreen` | 截图 | 尺寸/缩放/原点真实 | 像素内容（用 1×1 PNG 占位） |
| `ScriptedTransport` | HTTP | 响应字节完全可控 | 真实网络时序、TLS |
| `FakeTranscriber` | STT | 逐步返回预设 partial | 真实识别质量 |
| `FakeSpeech` | TTS | 立即"播完"或可取消 | 真实音频 |
| `FakeInjector` | 注入 | 记录文本与目标 | 真实应用行为 |
| `TestClock` | 时间 | 手动推进 | 时区夏令时 |

### 10 测试数据、版本与隔离

- 数据内联或 `tests/fixtures/`（文本/JSON，≤ 20KB）；不含真实人名、邮箱、密钥。
- 图片用程序生成的 1×1 PNG（常量字节数组）。
- 每个测试独占目录（`tempfile::TempDir`）。

---

## C. 专项质量测试

### 11 数据一致性、并发、故障与恢复

| 用例 | 断言 |
|---|---|
| TC-STO-001 事务回滚 | 故意在 `commit_turn` 中触发约束冲突 → messages/usage 均未写入 |
| TC-STO-002 迁移幂等 | `migrate()` 连续两次 → `user_version` 不变、无错误 |
| TC-STO-003 删除传播 | 删库后目录内无 `*.db`/`memory/*`/`workspace/*` |
| TC-AGT-004 崩溃恢复 | 运行中丢弃进程（模拟：只落检查点）→ `recover_interrupted` 标 `Paused` |
| TC-AGT-005 看门狗 | `heartbeat` 21 分钟前 → 自动 `Paused(inactivity)` |
| TC-AGT-007 重放保护 | 恢复后不重复执行 `shell`/`fs_delete` |

### 12 安全、权限与隐私

| 用例 | 断言 |
|---|---|
| TC-PRIV-001 采集门禁 | 未开 gate → `FakeScreen.calls == 0`（编号与矩阵一致） |
| TC-SEC-001 脱敏 | 含 Bearer、api_key=、40 位 hex 的日志行 → 全部替换 |
| TC-SEC-003 沙箱 | `../../etc/passwd`、绝对路径、符号链接逃逸 → `Denied` |
| TC-SEC-004 SSRF | `127.0.0.1`、`127.1`、`2130706433`、`0x7f.0.0.1`、`169.254/16`、`100.64/10` → `Denied` |
| TC-SEC-005 注入守卫 | 终端单行；密码类在 tick 与 finish 都不注入 |
| TC-SEC-006 删除报告 | 八步逐项报告，失败必须出现 |

### 12.1 首跑与交办（F1）

| 用例 | 断言 | 状态 |
|---|---|---|
| TC-ONB-001 保存 key 与模型 | `save_provider` 后 `app_state.ready=true`；`config.toml` 含 `chat_model`；钥匙串可读回 | spec-only（无桌面自动化） |
| TC-ONB-002 拉模型失败要说人话 | 错 key → `E_AUTH` 文案，不白屏 | spec-only |
| TC-TASK-001 桌面可交办 | `create_task` 返回 id，任务出现在列表并进入 running | spec-only |
| TC-TASK-002 批准后继续 | waiting_approval 时 `approve_task` 触发执行 | spec-only |
| TC-WIN-001 关窗可召回 | 关闭主窗后进程仍在，再次激活可显示 | spec-only |

**这五条尚无自动化测试**，只有手工冒烟；G2-WP6 因此不计入通过。

### 12.2 麦克风权限三态（平台层）

麦克风不是布尔。未决定靠系统第一次弹框解决，已拒绝只能去设置里改。这条区别决定界面给哪个按钮，所以它必须被证明，而不是靠读代码。

| 接口 | 断言 | 证据 |
|---|---|---|
| `MicrophoneAccess::grants_input` | 只有 `Granted` 算有权限 | `crates/platform/src/permissions.rs::only_granted_counts_as_present` |
| `MicrophoneAccess::as_str` | 三态序列化成 `not_determined` / `denied` / `granted`，界面据此判断 | 同上 |
| `missing_microphone` | 未决定与已拒绝都算缺，但两者文案不同 | `::a_microphone_never_asked_is_not_the_same_advice_as_one_refused` |
| `request_microphone` | 输入流打不开时不得报告为已授予 | `::requesting_the_microphone_reports_whether_the_stream_opened` |
| `plan_request` | 未决定给弹框、已拒绝给设置、已授予不动作、未知名字不算处理过 | `crates/platform/src/permissions.rs::each_state_of_the_microphone_leads_to_the_right_action`、`::an_unknown_permission_name_is_not_treated_as_handled` |
| `request` | 决策与副作用分开，决策可离线断言 | 同上（`plan_request` 是被测的那一半） |
| `PermissionOutcome::as_str` | 三种结果序列化成窗口读的字符串 | `::permission_outcomes_serialise_to_the_strings_the_window_reads` |
| `MicrophoneAccess::as_str` | `not_determined` / `denied` / `granted` 三个字面量 | `::the_three_microphone_states_keep_their_wire_names` |

`microphone_access()` 读 `+[AVCaptureDevice authorizationStatusForMediaType:` 的三个返回值，无法离线断言；它的手工验证方式是真机首次运行看到系统弹框，证据记在 #63。

### 12.3 key 与模型在保存前就被证明（#64）

一条规则。任何写进钥匙串或配置文件的东西，必须先在一个不落盘的路径上通过验证。坏 key 与不存在的模型名都在这里被拦住；拦住时旧配置保持不变。

| 接口 | 断言 | 证据 |
|---|---|---|
| `check_save` | 401 的 key 在保存前就失败 | `crates/core/tests/providers.rs::a_refused_key_fails_the_save_check` |
| `check_save` | 模型名不在列表里就拒绝，且提示里带那个名字 | `::a_model_outside_the_list_is_refused_before_it_is_saved` |
| `check_save` | 还没选模型也要验 key（列表能取到才算过） | `::an_unchosen_model_still_checks_the_key` |
| `build_with_key` | 用候选 key 构建 provider 不写钥匙串（写入即 panic） | `::a_candidate_key_is_used_without_being_stored` |
| `describe_target` | 提示里有 provider 名与 base_url | `::the_target_is_named_so_a_refusal_points_at_one_endpoint` |
| `save_provider` | 被拒的保存不改写还能用的配置文件，也不写钥匙串 | `::a_refused_save_leaves_the_working_config_file_alone` |
| `save_provider` | 通过的保存写模型并存入 key | `::an_accepted_save_writes_the_model_and_stores_the_key` |
| `plan_injection` | 终端永不逐字敲，只粘贴压平后的文本 | `crates/core/src/text.rs::a_terminal_never_receives_typed_text` |
| `plan_injection` | 有意的听写才走键盘 | `crates/core/src/text.rs::an_editor_receives_keystrokes_when_the_text_was_asked_for` |
| `plan_injection` | 兜底回答只复制，永不敲 | `crates/core/src/text.rs::a_fallback_answer_is_copied_and_never_typed` |
| `TextInjector::copy` | TTS 失败时不在键盘路径注入，回答进剪贴板 | `crates/core/tests/e2e_ask.rs::silent_tts_falls_back_to_text_and_clipboard` |
| `conversation_session` | 30 分钟内两次提问共用一个 session，第二次请求带上一轮 | `crates/core/tests/e2e_ask.rs::a_follow_up_shares_the_session_and_sees_the_earlier_turn` |
| `conversation_session` | 间隔超过窗口就换新 session | `crates/core/tests/e2e_ask.rs::a_long_gap_starts_a_new_conversation` |
| `apply_dictionary` | 不在长词内部替换，且一次扫描不级联 | `crates/core/src/dictation.rs::a_dictionary_key_does_not_reach_inside_a_longer_word`、`::a_dictionary_rewrite_does_not_feed_the_next_entry` |
| `looks_secret` | 裸的 6-8 位数字不再被当验证码（订单号可听写） | `crates/core/src/dictation.rs::a_bare_long_number_is_no_longer_a_code` |
| `looks_secret` | 说了"验证码/code"再加 6-8 位数字仍然拦；卡号仍按 Luhn 拦 | `::a_code_someone_names_is_still_a_code`、`::a_card_number_is_still_refused` |
| `secret_reason` | 提示能说清是哪条规则命中 | `crates/core/src/dictation.rs::the_notice_can_name_which_rule_fired` |
| `dictation_shortcut` | `fn+control` 降级到 `ctrl+alt+d` 并标记"不是用户要的" | `apps/desktop/src/main.rs::the_default_dictation_binding_falls_back_because_tauri_has_no_fn` |
| `dictation_shortcut` | Tauri 能表达的绑定按原样使用 | `apps/desktop/src/main.rs::a_binding_tauri_can_express_is_used_as_written` |
| `tauri_binding` | 双击手势不是全局快捷键；修饰键名翻译正确 | `::a_double_tap_gesture_is_not_a_global_shortcut`、`::modifiers_are_translated_to_the_names_tauri_parses` |
| `install_dictation_shortcut` | 按住出字、松手收尾、预算耗尽停 ticker | spec-only（需真人按键与真麦克风） |
| `reregister_dictation` | 保存后按新配置重注册，旧键解绑 | spec-only（需真人改配置后按键） |
| `durable` | 文件存储持久；钥匙串拒写后不持久；两者组合的四种取值 | `crates/platform/src/secrets.rs::durability_is_decided_by_the_file_store_and_the_keychain` |
| `is_durable` | 只剩内存时不得自称已保存 | `::a_store_that_cannot_persist_does_not_call_itself_durable` |
| `allow_file_store` | 显式落盘后新进程仍读得到（文件即开关） | `::an_opted_in_store_is_still_opted_in_after_a_restart` |
| `set-key` | 钥匙串拒写时报错退出，不打印 saved | 手工：`printf '"'"'k'"'"' \| openclicky --data-dir /tmp/x set-key` → exit 1 |
| `set-key` | 错 key 报 `E_AUTH` 并带上 `provider @ base_url` | 手工：同上，`--remember-file` 指向真实端点 |

### 13 性能、容量、稳定性与成本

**负载模型**（写清才能比较）：单用户、串行会话、4K×2 显示器、12s 语音、假 provider 固定延迟 300ms。
本地脚本（不进 CI 门槛，**尚未实现**）：计划提供 `scripts/bench-capture.sh` 与 `openclicky bench` 子命令；实测前本节只定义口径，不声称有数据。
**统计口径**：失败请求计入分母；去掉最慢 1% 后再算 p95 视为不同指标，必须标注。断言仅"不超时"（模型假实现延迟 0 的纯逻辑部分）。

### 14 体验、多端兼容

| 用例 | 断言 |
|---|---|
| TC-UI-001 渲染计划纯函数 | 同一 `plan_id` 生成相同路径（确定性抖动种子） |
| TC-UI-002 胶囊/throttle | 连续 8 次未点 → 暂停 1 小时 |
| TC-PLAT-001 三平台编译 | CI matrix `cargo build --workspace` |

### 15 安装、升级、迁移与运行验收

| 用例 | 方式 |
|---|---|
| TC-INST-001 macOS 安装 | 手工：dmg → Applications → 打开 → 授权 → 按键 |
| TC-INST-002 Windows 安装 | 手工：exe → More info → Run anyway → 授权 |
| TC-INST-003 Linux 安装 | 手工：AppImage chmod +x / deb dpkg -i |
| TC-UPG-001 升级保留数据 | 手工：v2.0.0 写入数据 → 装 v2.0.1 → 数据仍在 |

---

## D. 自动化、证据与缺陷

### 16 自动化策略、回归与测试自身质量

- 每个 PR：全量 `cargo test`（秒级），禁止只跑改动模块。
- 测试自身质量检查：断言必须能失败（每个 P0 用例做一次"故意破坏实现"验证，记录在 PR）。
- 禁止 `assert!(true)`、禁止捕获所有错误后不处理、禁止 `unwrap_or_default()` 掩盖失败。

### 17 执行记录、证据包与结果报告

证据 = CI 运行链接 + 命令输出摘要 + （G2/G3）冒烟记录。报告模板见 `06-acceptance.md`。

### 18 缺陷管理、修复确认与回归

缺陷必须带：复现用例（先加测试）、根因、修复 commit、回归范围。修复后**先看到测试变绿**，再看代码。

---

## E. 验收、责任与维护

### 19 进入、通过、退出与发布规则

| 阶段 | 条件 |
|---|---|
| 进入 G1（PR 评审） | 测试已写、CI 已跑 |
| G1 通过 | P0 全绿 + 无新增警告 + 已测量的覆盖率不降（未接入时记未测）+ `scripts/tds-evidence.py` 通过（没有引用不存在的测试） |
| 进入 G2 | 工作包 PR 全合并 + 检查点通过 |
| G2 通过 | 该包内**全部 TC 状态为「已实现」且执行通过** + 手工冒烟记录 + 证据绑定 commit；任何 `部分`/`spec-only` 都计未覆盖，不得判通过 |
| 进入 G3 | 全部 G2 通过 |
| G3 通过 | **tech-prd §18 的九条全部满足**（L1–L3 全绿+覆盖率、clippy/fmt、三平台构建、三平台安装冒烟、INV-1…8 有用例且通过、G2 结论绑定同一 commit、未测项清单且无 P0/P1 未测项、迁移与恢复演练、05-deployment 与产物一致） |
| 退出（可不通过） | 未测项显式列出时，只能评"有条件通过"，且必须写清风险与补救期限 |

### 20 角色、资源、变更与维护规则

| 项 | 规定 |
|---|---|
| 维护人 | 人类维护者 |
| 变更 | 期望改变 → 先改本文 + 说明理由 → 再改测试 |
| 例外 | 记录在 `06-acceptance.md`，含到期日 |
| 复审 | 每个 G3 前复核本文件与代码是否仍然一致 |

---

## AI / Agent 系统专项

### 21 测整个系统，不只模型

断言链：`输入 → 提示词构造 → 请求体（含图片/tools）→ 响应解析 → 状态 → 用量 → 持久化 → 事件序列`。任一段偏离即失败。

### 22 同时检查结果、过程约束与禁止行为

| 类型 | 例子 |
|---|---|
| 结果 | 任务产物文件存在且内容正确 |
| 过程约束 | 工具调用顺序、步数 ≤ 30、每步有检查点 |
| 禁止行为 | 未批准执行 `fs_delete`；`shell` 关闭时被调用；写入工作区外 |

### 23 数据集、重复运行与统计口径

假 provider 脚本固定，运行可重复（无随机、无网络、无时间依赖）。路由/计费类用表驱动全枚举，不抽样。

### 24 评分器也需要验证

`annotations` 解析器自身用"反例集"验证（畸形标签必须被丢弃而不是猜测）；`merge_partial` 用性质测试（输出必须是输入的公共前缀）。

---

## 完整用例示例（实例，已实现）

**TC-AGT-002 工具被拒绝后不得执行，且不得重复询问 30 分钟**

| 字段 | 内容 |
|---|---|
| 层级 | 集成 |
| 依据 | tech-prd §12 SEC-4、SRC-PRD K4 |
| 前置 | 任务运行中；模型脚本第 2 步返回 `fs_delete` 调用 |
| 输入 | 用户点 "Not now"（`Approvals::deny(now)`） |
| 步骤 | `runner.step()` 两次（第二次仍返回同一调用） |
| 预期 | 第一次 → `WaitingApproval`；`FakeTool.calls==0`；拒绝后 → 不再询问、直接 `Denied`；30 分钟后 → 再次询问 |
| 证据 | `tests/e2e_agent.rs` |
| 不能证明 | 真实 UI 是否把蓝色卡片画对 |

---

## 判据自检

| 问题 | 答案位置 |
|---|---|
| 测什么、为什么测 | §02、§03 |
| 怎样测、在哪里测 | §04、§05、§09 |
| 用什么数据 | §10 |
| 凭什么判定对错 | §06 |
| 留下什么证据 | §17 |
| 什么条件下接受这个版本 | §19 |
| 还有哪些风险没排除 | §03、§19 退出条件 |

---

# 附录 A：v1.1 补全（评审清单逐项）

## A-06 判定依据的独立性

| 被测对象 | 允许的"预期来源" | 禁止的"预期来源" |
|---|---|---|
| 坐标换算 | 手算的全局坐标常量 | 调用被测的 `to_global` 再比较 |
| 用量 | 手写期望单位数 | 读取 `usage.state()` 反推 |
| 任务完成 | 产物文件存在 + 独立内容断言 | 模型自述"完成" |
| 重试 | 传输层记录的请求次数 | 日志里"已重试"字样 |
| 不越权 | 假工具 `calls` 为空 | 返回体里的 `denied` 字段 |

## A-07 用例规格实例（12 字段齐全）

**TC-AGT-002 拒绝后不执行、30 分钟内不重复询问**

| 字段 | 内容 |
|---|---|
| 编号与名称 | TC-AGT-002 拒绝后不执行且不重复询问 |
| 目的与关联 | 验证 SEC-4/K4；风险：越权删除 |
| 类型与责任人 | 集成；实施 Agent 维护，验收负责人复核 |
| 版本与环境 | 被测 commit；`:memory:` store；假工具；`TestClock` |
| 前置条件 | 任务 Running；已到第 2 步；模型脚本返回 `fs_delete` |
| 输入与数据 | `Approvals::deny(now)`；同一工具调用重复两次 |
| 操作步骤 | 1) `step()` → 2) `deny()` → 3) `step()` → 4) 推进 30 分钟 → 5) `step()` |
| 预期结果 | (1) `WaitingApproval`、`FakeTool.calls==0`；(3) 直接 `Denied` 不再询问；(5) 再次询问 |
| 判定规则 | 精确断言状态与调用次数；时间窗口 30 分钟 ±0 |
| 证据要求 | 断言输出 + `task_steps` 行 |
| 清理与恢复 | TempDir 自动清理；无后台线程 |
| 自动化关联 | `tests/e2e_agent.rs::rejection_blocks_and_backs_off` |

## A-17 结果状态与分母

每次运行必须落到五种状态之一：**通过 / 失败 / 被阻塞 / 未执行 / 无法判定**。

- 报告必须给分母：`通过 42 / 计划 45（阻塞 2，未执行 1）`。
- **系统按要求返回"结果未知"算通过**；测试者拿不到证据才是"无法判定"。
- 跳过需要原因与批准人，记入 `06-acceptance.md`。
- flaky 处理：首次失败与全部重试都保留；同一用例两次重试才通过 → 标记 `flaky`，按缺陷处理，不得静默通过。

## A-3.3 Agent 评测口径（task vs trial）

| 项 | 规定 |
|---|---|
| task | 一个脚本化场景（固定模型响应序列） |
| trial | 同一 task 的一次运行 |
| 报告 | 每 task 运行次数、通过数、违规数、超时数 |
| 表述 | 禁止用"最好一次"代表能力；用 `n/N 通过` |
| 违规 | 任何禁止行为（越权/工作区外写/未批准 shell）出现 1 次即该 task 失败 |
| 局限 | 未观察到违规只说明"这些运行中未观察到"，不是保证 |

## A-19 关键约束的阻断规则

| 约束 | 处理 |
|---|---|
| INV-D1（未按键采集） | 任何失败 → 阻塞发布，不得用总通过率抵扣 |
| INV-D6（密钥泄漏） | 同上 |
| INV-D7（工作区逃逸） | 同上 |
| INV-D5（非幂等重放） | 同上 |
| 其他 P1 | 可"有条件通过"，须写残余风险、责任人、有效期 |

**因时间/预算耗尽停止测试 ≠ 测试目标达成。**

## A-20 本轮自查与修正

| 评审意见 | 修正 |
|---|---|
| 预期结果可能来自被测实现 | A-06 判定依据独立性表（禁止来源） |
| 用例字段不全 | A-07 给出一条 12 字段完整实例 |
| 汇总可能美化 | A-17 五状态 + 公开分母 + flaky 保留 |
| Agent 评测口径缺失 | A-3.3 task/trial + n/N |
| 关键约束可被总通过率抵扣 | A-19 阻断规则（INV 类直接阻塞发布） |

**仍未覆盖（已知）**：真实供应商在线行为、真实 TCC 弹窗、Wayland、跨设备迁移（非目标）。
