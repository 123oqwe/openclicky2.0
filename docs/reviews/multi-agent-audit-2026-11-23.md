# 多 Agent 严格复核报告（2026-11-23）

| 项 | 内容 |
|---|---|
| 触发 | 用户要求"用多 agent 去严格检查，越清楚越好" |
| 标准 | 对话原文 `docs/reviews/standards/source-conversation.md`（六部分，未改写） |
| 审查员 | 7 个独立 Agent（技术 PRD / ADR / SDD / playbook / TDS / 跨文档 / 代码 vs 规格） |
| 反证 | 25 个独立怀疑者，每条发现单独打开原文或 `feat/impl` 代码核对 |
| 规模 | 发现 198 条；确认 192、推翻 6 |
| 分布 | P0 11、P1 102、P2 62、P3 17 |
| 原始数据 | `docs/reviews/audit/findings-2026-11-23.json`（含每条的证据、影响、建议、反证意见） |

## 结论（先看这段）

**之前的 "PASS WITH RISK" 过于宽松，不成立。** 22 个条目确实都有章节，但严格核对后：

1. **文档内部/文档之间有 11 个 P0 级契约冲突**（同一事实两处不同值，或承诺了设计兑现不了的保证）；
2. **代码实现与已验收的规格有系统性偏差**：门禁、权限、计费、事务、状态机五处"规范说有、代码里没有执行点"；
3. **验收证据本身有问题**：TDS 追踪矩阵引用了 7 个不存在的测试文件，两个安全用例是空转；
4. **被 15+ 处引用的交付物不存在**：`06-acceptance.md`、`05-deployment.md`、CI workflow、`check-invariants.sh`。

因此当前状态应记为：**文档 vN 未通过严格验收；已实现的 core 是无门禁/无权限管控的原型，不满足 INV-1/3/4/5/7**。这比"PASS WITH RISK"更接近事实，也更难听，但它是这次复核的价值。

## P0：11 条（阻断发布，必须修）

| 编号 | 位置 | 问题 | 影响 | 建议修法 |
|---|---|---|---|---|
| P0-01 | `docs/adr/ADR-0006-annotation-protocol.md` §07 决策结论 第 1 条规则（第 67 行） | 坐标换算公式缺“模型图→原图”的降采样比，按 ADR-0003 的 1280 缩放，4K/Retina 屏指向会偏 3 倍。 | 这是 Accepted 的坐标契约，PRD TR-12 原样抄了同一公式，三份文档一起错。DisplayInfo 只带 scale，实现方拿不到缩放比；3840px 缩到 1280 时，模型给的 x=640 应落在逻辑 960，公式算出 320。 | 把“模型图像素”显式改为两段式：img_px × (原图长边/1280) → orig_px，再 /scale；`to_global` 增加 shot 尺寸（或 scale 与 downscale 两个参数），同步修改 ADR-0003 §07、PRD TR-12、SDD §04.2。 |
| P0-02 | `docs/adr/ADR-0003-screen-capture.md` 标题第 1 行、§07 第 1 条、§09 剩余风险（第 54、73 行） | ADR 自称“只在按键瞬间采集”，却授权非按键采集理由与“引导里预热一次”，与 C-4、INV-1、TR-02 冲突。 | 门禁的唯一价值是“不按键不看屏”可证明（INV-1 / TC-PRIV-002 的计数口径）。这两处给了绕过路径，而文档未定义 reason 与“用户按键”的等价关系，标题承诺即为设计无法兑现的保证。 | 删除“预热”与 OnboardingDemo/UserRetry 的独立采集权，或明确它们必须由真实用户按键/点击触发并在 ADR 写 reason→授权来源映射；同步写清 INV-1 计数是否包含引导期。 |
| P0-03 | `docs/adr/ADR-0008-persistence.md` §07 决策结论 载体/权威表（第 59 行） | 把“最近截图”落盘 `cache/` 保留 24h，与 PRD“截图仅内存”及 ADR-0003“缓存上一帧 30s”三者互斥。 | 屏幕内容是最敏感资产（PRD §12.1 列为“极高/信任”）。落盘 24h 把“不留副本”改成“留一天”，却没有加密、删除归属、用户可见开关，三份文档给出三种留存语义。 | ADR-0008 明确截图为内存态（进程内 TTL，与 ADR-0003 的 30s 对齐）；若确需落盘，另开 ADR 记录理由、加密与删除责任，并同步 PRD §05.4 与 TR-02 的留存口径。 |
| P0-04 | `docs/01-tech-prd.md` 01:185 与 01:1049 | INV-5 称写入永不出工作区，TR-23 却允许批准后写工作区外，同一安全不变量被两条规范同时定义。 | 不变量与功能需求直接冲突：INV-5 阻断发布，则 TR-23 的批准路径必须删除；反之 INV-5 失效。实现者无法同时满足，验收也无判据。 | INV-5 收窄为“未获批准时不得写工作区外；批准仅对单一目标路径、一次性有效”，并在 §12.2 增加工作区外写入的第二道执行点（policy + fs canonicalize 复检）。 |
| P0-05 | `docs/01-tech-prd.md` 01:184 与 01:1122 | INV-4 保证删除后本机无残留，TR-28 承认系统备份/时光机副本无法清除，并且都没有执行点。 | 同一隐私承诺在两处给出互斥边界，无法判定 INV-4 是否通过；§08.3 删除顺序也未覆盖 TR-28 承认的保证外区域，报告口径会撒谎。 | INV-4 改为“应用数据目录与应用可控存储内无残留”，把系统备份/时光机列为明示保证外区域，并要求删除报告逐项列出该区域与用户可执行动作。 |
| P0-06 | `docs/02-sdd.md` §06:605、§15:768-785 vs 01-tech-prd.md:393-404 与 feat/impl:crates/core/src/store/mod.rs:521-556 | §15 无删除传播设计；§06 只写“删表+删文件”，tech-prd 的七步删除在 SDD 无对应，实现只删行。 | 隐私承诺“删干净”在执行点无覆盖（item 15 明确要求删除传播），且 §06 的“删表”与实现“删行”语义不同。 | 在 §06/§15 增七步删除传播表（与 tech-prd 编号对齐），定义 Memory/Secrets/Workspace/Log 的删除接口与逐步报告；改正“删表”为“删行”。 |
| P0-07 | `docs/02-sdd.md` docs/02-sdd.md:547-556（tasks DDL） vs docs/02-sdd.md:996 与 docs/01-tech-prd.md:281 | tasks DDL 无 epoch 列，但 A-12 与 tech-prd 都把 epoch 当作任务状态写权限与接管恢复的唯一保证。 | 00-baseline §4 把字段权威给 SDD，按 DDL 建库无法执行 A-12 的 `UPDATE tasks SET ... WHERE id=? AND epoch=?`，INV-D3 与"旧执行者恢复"在设计层不可实现。feat/impl 的 M001 已加 `epoch INTEGER NOT NULL DEFAULT 1`，说明文档落后于实现，两边的"权威"互相矛盾。 | SDD §05 DDL 补 epoch（及 A-12 用到的 runner_instance_id 若要落库），并在数据字典声明自增时机与写入校验；同时核对 tech-prd §22.2 的 Task 字段表。 |
| P0-08 | `docs/adr/ADR-0003-screen-capture.md` docs/01-tech-prd.md:235 vs docs/adr/ADR-0003-screen-capture.md:71 vs docs/adr/ADR-0008-persistence.md:59、docs/02-sdd.md:869 | 截图保留期三处不同值：tech-prd「仅内存 ≤24h」、ADR-0003「缓存上一帧 30s」、ADR-0008「cache/ 24h TTL」，SDD 又给了 24 小时落盘配置。 | "仅内存"与"磁盘 cache 24h"是互斥的数据落地契约，30s 与 24h 是互斥的保留期。这是 C-4 隐私底线（不按键不看屏）的延伸，也是删除传播与"删账号无残留"的一句依据；三个数并存意味着实现者可以自选最松的一份。 | 定一个口径（建议：仅内存、进程退出即丢，重绘缓存 30s），把 ADR-0008 的 cache/ 行与 SDD 的 keep_screenshots_hours 一并改掉，或新增 ADR 明确区分"重绘缓存"与"落盘缓存"并同步 tech-prd §05.4。 |
| P0-09 | `docs/02-sdd.md A-02 INV-D1 / §07；docs/01-tech-prd.md §12.2 SEC-1` crates/core/src/privacy.rs:44-48,66-70；crates/core/src/ports.rs:154-157；crates/core/src/voice.rs:120-125 | CaptureGate 只是计数器，捕获能力本身不受门禁约束：capture_all 无 gate 参数，任何持有 Arc<dyn ScreenSource> 的代码都能直接采集。 | INV-D1 号称由「privacy + 单入口」保证，但唯一入口是 voice.rs 里的一段约定写法；新增第二个调用点（例如任务里“看一眼屏幕”、缩略图预览）不会被编译器或类型系统拦住，隐私不变量在任何地方都未被强制。 | 把 capture_all 改成 `fn capture_all(&self, gate: &CaptureGuard<'_>)` 或引入 `CapturedShots` 只能由 `CaptureGate::open()` 产出的类型；FakeScreen 计数从 guard 里取，缺 guard 即编译失败。 |
| P0-10 | `docs/01-tech-prd.md §12.3；docs/02-sdd.md A-02 INV-D4` crates/core/src/agent/runner.rs:211,375-378；crates/core/src/ports.rs:431 | 「每次工具调用前重新检查授权与权限」在代码里没有执行点：runner 每步只查一次预算，全仓没有任何权限探测端口或调用。 | INV-D4 声称由 runner 保证，实际 runner 既不复检权限，也不在工具粒度复检预算；权限被用户在系统设置里撤销后，任务会继续按 Running 跑工具，而不是按 §12.3 进 WaitingApproval。 | 新增 `PermissionProbe` 端口（AX/CGPreflight/DISPLAY），在 runner.execute 每次工具调用前调用；返回 Denied 时按 §12.3 转 WaitingApproval 并写审计。 |
| P0-11 | `docs/02-sdd.md §11 任务状态机守卫行；docs/01-tech-prd.md §12.2 SEC-4` crates/core/src/tools/policy.rs:25-27,64-68,81-98 | Policy 对 Risk::External 与 Risk::Write 一律 Allow，`decide_write` 无生产调用点：规范要求 External(上传) 与 Write(区外) 都必须走批准。 | 写这一条就等于关掉 SEC-4 的一半：模型调用 fetch_url / http_get_json（数据外发）永不弹批准，Write(区外) 永远到不了审批路径（会被 fs 沙箱直接 Denied），文档对用户的「上传/越界写要你点头」承诺不成立。 | risk() 增加 outside_workspace 维度，Policy::decide 对 External 与 Write(区外) 返回 Ask（或 Ask/WaitingApproval），并让 runner 调 decide_write；补 TC-POL-002/003。 |

## P1 聚类（102 条，按主题）

| 聚类 | 说明 |
|---|---|
| SDD 表格与代码不一致（22 条） | SDD 的 DDL / 状态机 / 错误码 / 接口签名 / 事务描述，与 `feat/impl` 实际实现有系统性漂移（epoch 列、transition 表、19 vs 11 个错误码、record_step 事务、plan_from 签名）。 |
| 安全与权限没有执行点（约 10 条） | CaptureGate 只是计数器、policy 对 Write/External 恒 Allow、无权限探测端口、SSRF 只认点分四段、密码守卫只在 finish 生效。 |
| 计费与预算不可达（约 6 条） | runner 只发 AgentStep，长任务/10 秒豁免分支不可达；语音与听写不查预算；月度提额无法解除月额度。 |
| 验收证据造假或空转（约 8 条） | TDS 追踪矩阵把 7 个不存在的测试文件当证据；TC-SEC-001 与短按用例不做任何可失败动作；G3 门禁比 tech-prd 少 5 项。 |
| 缺失交付物（约 6 条） | `06-acceptance.md`、`05-deployment.md`、`.github/workflows/`、`scripts/check-invariants.sh` 被 15+ 处引用但不存在。 |
| 数字与口径冲突（约 10 条） | 8s vs 10s、重试 1 次 vs 2 次、首音 1000ms vs 2500ms、截图保留 30s/24h/仅内存、词典 json vs 表。 |

## P2/P3（79 条）

格式化、断链、编号、措辞、索引与正文不一致等；完整清单在 JSON。

## 被推翻的 6 条（避免误伤）

| 文档 | 原指控 | 推翻理由 |
|---|---|---|
| docs/04-test-design.md | 看门狗阈值写 21 分钟，与 tech-prd/02-sdd 的 20 分钟冲突 | TDS:160 'heartbeat 21 分钟前 → Paused(inactivity)' is a test input exceeding the 20-min threshold (tech-prd:1035, 02-sdd:69 |
| docs/adr/ADR-0009-secrets-privacy.md | ADR-0009 与 ADR-0003 都自称拥有“采集门禁”的权威，形成双权威且都未声明覆盖关系。 | ADR-0009 §07 makes no capture-gate decision (only title + tests/capture_gate.rs at :76); 适用范围 and §12 关联(ADR-0003) alrea |
| docs/01-tech-prd.md | 模型上下文限制“单次 ≤3 张图”与容量模型“截图上限 4 屏”冲突，4 屏时必有一屏无法送入模型且无规则说明。 | 01:576 is the capacity table (维度|假设|上限: 假设 2 屏, 上限 4 屏), not a per-request image requirement; 01:761 already states the  |
| docs/01-tech-prd.md | TASK 只有 7 步编号描述，没有状态机表：缺“谁可发起/守卫/副作用/非法转换”的显式定义。 | 02-sdd.md:678 "### 11 任务状态机（权威表）" has 当前/事件/新状态/守卫 rows incl. WaitingApproval, Paused(interrupted), retry; 00-baseline § |
| docs/01-tech-prd.md | §07.4 状态机含 Driving 态，但 TR-23 明确不实现 drive，该态无入口也无副作用定义。 | 01:356 Driving row and 01:1049 TR-23 quote exist, but "无入口" is refuted: 02-sdd.md §10 says "Driving 仅由任务产生，用户任何输入立即退出".  |
| docs/01-tech-prd.md | checkpoint 版本策略自相矛盾：一处“不识别 → Paused(incompatible)”，一处“未知字段忽略、保留已知”。 | 01:640 / 02-sdd A-20 "带 version；不识别 → Paused(incompatible)" reads as version mismatch (sibling row: 旧版本打开新 DB → 只读), whi |

## 修复顺序（先文档，后代码）

| 阶段 | 做什么 | 完成判据 |
|---|---|---|
| F1 | 修 11 个 P0 中的文档侧（截图保留、INV-4/5、epoch DDL、状态机、计费口径、错误码、词典权威、8s/10s、重试次数、首音预算） | 每条 P0 有对应 diff；`doc-audit` + `criteria-coverage` 仍全绿 |
| F2 | 建缺失交付物：`06-acceptance.md`、`05-deployment.md`、`scripts/check-invariants.sh`、`.github/workflows/ci.yml` | 文件存在且能运行；playbook 的"强制"项有工具载体 |
| F3 | 修代码 P0：CaptureGate 能力化、权限探测端口 + 每次工具调用前检查、Policy 覆盖 Write/External、删除传播、epoch 写入、计费可达 | 新增/更新测试先红后绿；`cargo test` 全绿 |
| F4 | 重写 TDS 的追踪矩阵：删除不存在的文件引用，标注"spec-only"，补齐失败即红的用例 | 每个 ✅ 都能 `git grep` 到对应测试名 |
| F5 | 重新出验收记录：把 PASS 改为"有条件通过"，逐条列出未验证项 | 新的 `docs/reviews/*` 与本次 JSON 对应 |

## 复现方式

```bash
python3 scripts/doc-audit.py          # 条目实质检查
python3 scripts/criteria-coverage.py  # 88 项编号覆盖
jq '[.[] | select(.severity_final=="P0")] | length' docs/reviews/audit/findings-2026-11-23.json
```
