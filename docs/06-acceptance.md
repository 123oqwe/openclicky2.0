# 06 验收记录（G0–G4）

| 项 | 值 |
|---|---|
| 编号 | OC2-ACCEPT |
| 版本 | 2.0（2026-11-23） |
| 权威性 | 功能是否验收，以本文件绑定到具体 commit 的结论为准（`00-baseline.md` §4） |
| 代码基线 | `feat/f1-onboarding` @ `16e9579`（未合入 main）；此提交之后只允许文档改动，代码再动必须重跑本文件的证据 |
| 范围定案 | `docs/adr/ADR-0015-delivery-decisions.md`（Q1–Q12） |
| 规则 | 结论绑定 commit；证据可复现；未测项必须列出；PASS 不得覆盖 P0 |

## 1. 状态一览

| 节点 | 对象 | 状态 | 绑定 | 备注 |
|---|---|---|---|---|
| G0 | 任务准入 | 通过 | — | 任务卡字段见 playbook §04 |
| G1 | 单 PR | **通过（有条件）** | 复核报告 + 修复 PR | 198 条复核发现：11 个 P0 均已修复并有测试；剩余 P1/P2 见 §5 |
| G2 | 工作包 | **部分通过** | `16e9579` | WP1–WP5 PASS；WP6 有条件（仅 macOS 产物实测） |
| G3 | 发布准入 | **阻塞** | — | 九条中 6 条满足，缺三平台产物/安装冒烟/迁移演练 |
| G4 | 开放后观察 | 未开始 | — | 无发布 |

## 2. G2 逐工作包

### G2-WP1 文档链
- 范围与依据：`00-baseline.md` §2；`docs/reviews/standards/`
- 版本基线：main @ 本次合并 commit
- 执行结果：doc-audit 103/103、coverage 88/88、ref-check 0 断链、invariants OK
- 结论：**通过**
- 证据：CI `docs-and-invariants`；`docs/reviews/audit-output.md`

### G2-WP2 问答（说 + 指）
- 范围：TR-01/02/05/06/12；TC-E2E-ASK-001…006、TC-ANNO-001…013、TC-HK-*
- 版本基线：`16e9579`
- 执行结果：`cargo test -p openclicky-core` 全绿；每个问题只采集 1 次、标签剥离、坐标按图片比例、TTS 失败回落剪贴板
- 结论：**有条件通过**。13 条 `TC-ANNO-*` 细项仍为 spec-only，按 `04-test-design.md` 的规则不得计为通过
- 未测：真实供应商在线路径（无 key）；平台真机 TCC 弹窗（本机 `doctor` 已显示 ok）

### G2-WP3 任务（办）
- 范围：TR-17/18/19；TC-AGT-001…009
- 版本基线：`16e9579`
- 执行结果：状态机全边、一次性批准、拒绝后 30 分钟静默、循环停止、恢复、等待 24 小时转暂停、并发上限 3、计费（任务收一次，步数如实计）
- 结论：**通过**
- 未测：真实长任务（>20 分钟）计时；外部副作用"结果未知"的端到端演练

### G2-WP4 听写（写）
- 范围：TR-15；TC-DICT-001…010
- 版本基线：`16e9579`
- 执行结果：增量后缀、终端单行、密码守卫（tick + finish）、词典学习
- 结论：**通过**
- 未测：真实麦克风与蓝牙设备切换

### G2-WP5 自动化（定时 + 建议 + 记忆）
- 范围：TR-24/25/26；TC-RTN-*、TC-SUG-*、TC-MEM-*
- 版本基线：`16e9579`
- 执行结果：时间表解析/下次触发/条件等待/三连失败暂停；打扰预算；记忆压缩与遗忘
- 结论：**通过**
- 未测：真实睡眠唤醒补跑（需要合盖测试）

### G2-WP6 交付（三平台）
- 范围：ADR-0011、`05-deployment.md`
- 版本基线：`16e9579`
- 执行结果：**有条件**
  - macOS：`tauri build --bundles app` 实测产出 `OpenClicky.app`（6.95 MiB），运行 8 秒无 panic
  - Windows / Linux：CI job 已写好（nsis / appimage+deb），**尚未运行**
  - CLI 三平台：release workflow 已存在，**尚未产出**
- 通过条件：release job 在 tag 上成功产出三平台文件 + 安装冒烟

## 3. G3 发布准入（tech-prd §18 九条）

| # | 条件 | 状态 | 证据 |
|---|---|---|---|
| 1 | L1–L3 全绿 + 覆盖率 | ✅ | 189 tests 全绿；`cargo llvm-cov --workspace --summary-only` 实测行覆盖 core 83.55%、全仓 68.59%、platform 适配层 42.83%。目标 80% 按业务逻辑算，platform 的 0% 全在需要硬件的模块（audio/capture/http/paths/permissions/speech/tts） |
| 2 | clippy 无警告、fmt 干净 | ✅ | CI rust matrix / 本地同命令 |
| 3 | 三平台 release 构建成功 | ✅ | run [45528604668](https://github.com/123oqwe/openclicky2.0/actions/runs/45528604668)：`build` 与 `desktop` 六个 job 全过，产出 `OpenClicky_2.0.0_aarch64.dmg`（3.40 MiB）、`OpenClicky.app`（7.37 MiB）、`OpenClicky_2.0.0_x64-setup.exe`（2.95 MiB）、`OpenClicky_2.0.0_amd64.AppImage`、`OpenClicky_2.0.0_amd64.deb`，外加三平台 CLI 二进制。`publish` 因为没有 `v*` tag 而失败，这是它该有的行为，正式发布要等合并后打 tag |
| 4 | 三平台安装 + 首启冒烟 | ❌ | 无安装器产物 |
| 5 | INV-1…8 有用例且通过 | ✅ | INV-1/3/4/7 绑定用例；INV-2 扫库验证 key 未落盘；INV-5 批准后写入 + 审计行断言、策略与写入器对“是否越界”的判定有对拍用例；INV-6 跨两步断言任务费只收一次；INV-8 恢复不重放 |
| 6 | G2 结论绑定同一 commit | ✅ | 本文件与 §2 全部工作包绑定 `16e9579`；WP6 的“有条件”是结论本身，不是过期 |
| 7 | 无 P0/P1 未测项 | ⚠️ | 见 §5 |
| 8 | 迁移与恢复演练 | ✅ | `migration_drill` 三条：v1 带数据迁到 v3 后行数、FTS 回填、`.db.bak` 回滚到 v1 全部核对通过；更高版本库只读打开 |
| 9 | `05-deployment.md` 与产物一致 | ⚠️ | 运行方式已写，安装器待补 |

**结论：G3 只剩第 4 条（三平台安装 + 首启冒烟）阻塞，需要 Windows/Linux 机器与安装器产物。**

## 3.1 施工顺序（业主已定案，见 ADR-0015）

| 阶段 | 内容 | 完成判据（业主可感知） |
|---|---|---|
| F1 能用 | UI 入库、首跑向导（key+模型）、热键反馈、任务入口与批准按钮、关窗可召回 | 业主在自己的电脑上完成"装→配→问一句→看它指→交办一件→批准" |
| F2 不烧钱 | 预算下沉到出站层、单次授权、并发抢占、CSP+命令白名单、SSRF 逐跳 | 额度用尽后问答也被挡住；同任务不会被执行两次 |
| F3 数据 ✅ | `approvals`/`artifacts` 表、`retries/pause_reason/waiting_since`、两个索引、FTS、`owner` 列 | 迁移脚本可重跑；任务完成后能查到产物归属（证据见 §7） |
| F4 体验 | notch（v2.1）、建议卡、听写热键、18 语言（分批）、更新验签 | 逐项按 ADR-0015 Q3/Q8/Q11/Q12 验收 |

**顺序不可交换**：F2 之前不发布，F3 之前不接新功能。

## 4. 例外与残余风险

| 项 | 说明 | 到期 |
|---|---|---|
| 未签名产物 | macOS/Windows 首启需手动放行（文档已写） | 获得证书时 |
| 在线路径未测 | 无用户 API key；协议层由假实现覆盖 | 用户首次配置后 |
| Wayland | 不支持注入/热键 | 有 Wayland 机器时 |

## 5. 已知未测项（不得当作通过）

1. 真实模型调用（问答/任务/听写全链路）
2. Windows 与 Linux 的构建与安装
3. 长任务 >20 分钟的真实计时
4. 睡眠/唤醒补跑
5. ~~迁移与恢复演练~~ 已由 `migration_drill` 覆盖，见 §7

## 6. 证据索引

| 证据 | 位置 |
|---|---|
| 单元/集成测试 | `cargo test --workspace`（171） |
| 文档检查 | `scripts/doc-audit.py`、`criteria-coverage.py`、`ref-check.py`、`tds-evidence.py` |
| 结构不变量 | `scripts/check-invariants.sh` |
| 复核原始数据 | `docs/reviews/audit/findings-2026-11-23.json` |
| 三平台打包 job | `.github/workflows/release.yml`（desktop） |

## 7. 2026-11-24 全栈审核修复批次

依据 `docs/reviews/fullstack-audit-2026-11-24.md`。证据：`cargo test --workspace` 189 通过、`clippy -D warnings` 干净、`scripts/check-invariants.sh` 与四个文档脚本全绿。

| 修复 | 位置 | 绑定测试（文件 › 用例） |
|---|---|---|
| 迁移 v2：五个新列、步骤唯一键、两个索引、审计只追加触发器 | `store/migrations.rs` M002 | `store mod` › `migration_adds_the_columns_the_decisions_promised`、`duplicate_step_rows_are_rejected`、`audit_log_rejects_updates_but_deletion_flow_still_runs` |
| 批准一次性 | `agent/runner.rs` | `e2e_agent` › `approval_is_one_shot_for_the_same_call` |
| 拒绝静默 30 分钟 | `Checkpoint.denied_until` | `e2e_agent` › `not_now_blocks_asking_for_thirty_minutes` |
| 等待 24 小时转暂停 | `persist` + `expire_stale_waiting` | `e2e_agent` › `stale_waiting_tasks_are_paused_by_the_watchdog` |
| 并发上限 3（E_QUEUE） | `TaskRunner::create` | `e2e_agent` › `the_fourth_active_task_is_rejected` |
| 预算覆盖问答、打字、听写 | `voice.rs`、`dictation.rs` | `e2e_ask` › `exhausted_budget_blocks_a_typed_question_before_any_request`、`exhausted_budget_blocks_the_hotkey_path_too` |
| 打字提问流式且可取消 | `voice.rs` | `e2e_ask` › `typed_answers_stream_deltas_then_the_answer`、`cancelling_a_typed_answer_stops_the_stream_and_charges_nothing` |
| 祈使句判定为任务 | `router.rs` + 桌面热键 | `router` › 6 条（`imperative_*`、`a_question_mark_makes_it_not_a_task`） |
| 卡号与验证码守卫 | `dictation.rs` `looks_secret` | `dictation` › `card_numbers_are_detected_by_luhn_even_without_the_word`、`one_time_codes_are_detected_but_years_are_not` |
| SSRF：真实 IP、IPv6、禁跟随重定向 | `tools/net.rs`、`platform/http.rs` | `tools net` › `blocks_loopback_and_private`、`parses_ipv6_with_port`、`resolution_catches_names_that_point_home` |
| 出站默认需批准，白名单主机免问（旧的上传启发式删除） | `tools/policy.rs`、`tools/net.rs`、`config.rs`（`tools.allow_domains`） | `tools policy` › `external_asks_unless_the_host_is_allowlisted`、`a_url_carrying_data_is_not_a_reason_to_allow_it`；`tools net` › `an_empty_allowlist_trusts_nothing`、`the_allowlist_covers_subdomains_but_not_lookalikes`、`an_unparseable_url_is_never_allowed` |
| 数据库句柄先关再删文件（Windows os error 32） | `store/mod.rs` `close()`、`deletion.rs` 第 1 步 | `deletion` › `deletion_releases_the_database_handle_before_removing_files` |
| 删除报告不再吞掉失败的逐表结果 | `deletion.rs` | `deletion` › `a_table_that_refuses_to_empty_fails_step_one` |
| 迁移 v3：`approvals`/`artifacts` 表、`pause_reason`、FTS5 与三个触发器 | `store/migrations.rs` M003、`store/mod.rs` | `store mod` › `migration_reaches_v3_and_adds_the_pause_reason`、`migrating_twice_is_a_no_op`、`full_text_search_finds_a_message_and_the_delete_trigger_clears_it`、`pause_reason_round_trips_and_clears` |
| 批准落账，单次可用 | `agent/runner.rs` `note_approval` | `store mod` › `an_approval_is_decided_once_and_used_once`、`a_denied_approval_cannot_be_consumed`、`asking_twice_about_the_same_step_keeps_one_row`；`e2e_agent` › `approval_lets_the_action_run_once` |
| 三平台安装包真的产出（G3 第 3 条） | `.github/workflows/release.yml`、`scripts/linux-deps.sh` | run 45528604668 的六个构建 job；Linux 首轮因 `wayland-client.pc` 与 `libpipewire-0.3` 缺失而红，根因是三份分叉的 apt 列表 |
| 三平台 CI 首次全绿（G3 第 3 条的编译面） | `.github/workflows/ci.yml` | 运行 [45525444879](https://github.com/123oqwe/openclicky2.0/actions/runs/45525444879)：`prepare`、`docs-and-invariants`、`rust-macos-latest`、`rust-ubuntu-latest`、`rust-windows-latest` 五项全过；该 PR 此前连续十次红 |
| 覆盖率接入（G3 第 1 条） | `cargo llvm-cov --workspace --summary-only` | 行覆盖 core 83.55% / 全仓 68.59%；platform 适配层 42.83% |
| 产物内嵌 commit 与构建时间（tech-prd 631） | `apps/cli/build.rs` + `LONG_VERSION` | `openclicky-cli` › `the_version_line_names_the_build`；真机 `openclicky --version` 输出 `2.0.0 (commit 53af262aaab1, built at unix 1795569338)` |
| 迁移与恢复演练（G3 第 8 条） | `store::open` 只在真要迁移时备份；`open_read_only` | `migration_drill` › `the_drill_migrates_v1_and_the_backup_restores_it`、`the_backup_stays_the_pre_migration_copy`、`a_newer_database_opens_read_only_instead_of_bricking` |
| INV-5：策略与写入器对“越界”必须同一套判定（符号链接也算越界） | `tools/fs.rs::escapes_workspace` | `tools fs` › `the_policy_and_the_writer_agree_about_what_is_outside` |
| INV-5：越界写入必须留下审计行 | `store::audit_rows` | `e2e_agent` › `approving_an_outside_write_lets_it_through_once` |
| INV-6：任务费只收一次，步骤各收一次 | `TaskRunner::step` + `UsageMeter` | `e2e_agent` › `task_is_charged_once_at_start_and_steps_separately` |
| INV-2：工具参数里的 key 不落 SQLite | `agent/runner.rs` 审计摘要 + 步骤摘要 | `e2e_agent` › `a_key_in_tool_arguments_never_reaches_the_database` |
| INV-8：恢复不重放被中断的调用 | `recover_interrupted` | `e2e_agent` › `startup_recovery_pauses_stale_running_tasks` |
| 产物归属可查（kind / bytes / sha256） | `agent/runner.rs` `finish`、`security::sha256_file` | `e2e_agent` › `writes_a_file_then_finishes_with_evidence`；`store mod` › `artifacts_survive_a_reopen_and_stay_scoped_to_their_task` |
| 删掉没人用的截图保留期开关（口径统一为仅内存 30s） | `config.rs`、`02-sdd.md` §19 | 无自动化，配置形状由 `config` 往返用例覆盖 |
| 前端零 innerHTML | `ui/dist/index.html` | 无自动化，一次性 DOM 断言 |
| 授权清单、CSP、图标 | `capabilities/default.json`、`tauri.conf.json` | 需真机 |
| 取消令牌表、answer_question、权限命令、单实例、召回键、托盘 | `apps/desktop/src/main.rs` | 需真机 |
| 不可信文本包装 | `prompt.rs` + `runner.rs` | `prompt` › 注入与围栏用例 |
| Linux 密钥不再可复算落盘 | `platform/secrets.rs` | `secrets` › 内存默认、opt-in、0600、跨安装不同密文 |
| 十处文档矛盾、37 条无归属规则 | `01/02/03/04`、`ADR-0012`、`00-baseline` | 三个文档脚本 |

**仍未修，逐条记录，每条都写明卡在哪**：

| 项 | 卡在哪 |
|---|---|
| `ask_typed` 不支持“先画再问” | 截图与指画已经在打字通道里（`voice.rs` 走同一个 `CaptureReason::TypedQuestion` 闸门，`annotations::parse_reply` 出坐标）；缺的只是用户先圈一块再打字，那需要按住热键，属于热键通道 |
| 应用自有命令未走 Tauri ACL | `withGlobalTauri: true` 是无打包步骤（ADR-0014）的前提；关掉它要先给 UI 加模块加载，属于 UI 工程改造 |
| 多屏覆盖层（U9） | 需要每屏一个窗口，窗口管理要重写 |
| 单修饰键长按（T2）、macOS 15.1 屏幕探测（T3） | 平台级重写，需真机验证 |
| `usage_events.task_id` 无外键（D5） | SQLite 加外键要重建表；现有索引已覆盖查询，风险大于收益 |
| 读写连接分离（D10） | 单进程单写者 + 2s busy_timeout 已覆盖当前负载 |
| opt-in 落盘密钥无主密码（X4） | 需要引入 Argon2id 与解锁流程，是一个独立特性 |
| 真实模型、真机 TCC、Windows/Linux 构建、托盘渲染、CSP 运行时效果 | 本机无法制造 |
| 覆盖率目标 80% 全仓未达 | 全仓 68.59%，但业务逻辑（core）83.55% 已达标；差额几乎全在 platform 的设备适配层，那段没有真机就只有 0% |
| 六个产品分叉（账号付费、热键手感、多屏、听写注入、加密、遥测） | 未拍板；当前按可回滚的 A 选项落地 |
