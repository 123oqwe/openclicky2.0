# 文档验收：机器检查结果

## 第 1 步 · 技术实现 PRD

| 条目 | 机器检查 | 缺失标记 |
|---|---|---|
| 01 基线/负责人/范围 | PARTIAL | 文档编号, 适用发布 |
| 02 背景/约束/假设/目标/非目标 | PASS | — |
| 03 产品→技术追踪 | PASS | — |
| 04 质量目标与技术保证 | PASS | — |
| 05 边界与总体架构 | PASS | — |
| 06 子系统划分与所有权 | PASS | — |
| 07 流程/状态机/并发 | PASS | — |
| 08 数据架构与一致性 | PASS | — |
| 09 接口与契约 | PASS | — |
| 10 客户端与本地状态 | PASS | — |
| 11 选型与证据 | PASS | — |
| 12 安全架构与隐私 | PASS | — |
| 13 故障模型 | PASS | — |
| 14 性能与容量 | PASS | — |
| 15 可观测性 | PASS | — |
| 16 环境与供应链 | PASS | — |
| 17 迁移与升级 | PASS | — |
| 18 验证与准入 | PASS | — |
| 19 成本与预算 | PASS | — |
| 20 实施分解 | PASS | — |
| 21 风险/债/索引/变更 | PASS | — |
| 22 AI/Agent/桌面专项 | PASS | — |

## 第 2 步 · ADR（15 份）

| 条目 | 机器检查 | 缺失标记 |
|---|---|---|
| ADR-0001-desktop-stack.md | PASS | — |
| ADR-0002-local-first.md | PASS | — |
| ADR-0003-screen-capture.md | PASS | — |
| ADR-0004-input-injection.md | PASS | — |
| ADR-0005-model-access.md | PASS | — |
| ADR-0006-annotation-protocol.md | PASS | — |
| ADR-0007-agent-runtime.md | PASS | — |
| ADR-0008-persistence.md | PASS | — |
| ADR-0009-secrets-privacy.md | PASS | — |
| ADR-0010-usage-budget.md | PASS | — |
| ADR-0011-distribution.md | PASS | — |
| ADR-0012-testing.md | PASS | — |
| ADR-0013-dictation-streaming.md | PASS | — |
| ADR-0014-desktop-shell.md | PASS | — |
| ADR-0015-delivery-decisions.md | PASS | — |

## 第 3 步 · SDD

| 条目 | 机器检查 | 缺失标记 |
|---|---|---|
| 01 标识/版本/范围/状态 | PARTIAL | 已批准 |
| 02 需求分配与约束 | PARTIAL | 不变量 |
| 03 结构分解与共用约定 | PASS | — |
| 04 单元详细设计 | FAIL | 前置条件, 后置条件, 副作用, 资源责任 |
| 05 数据模型与字典 | PASS | — |
| 06 存储与事务 | PASS | — |
| 07 同步接口契约 | PARTIAL | 重复 |
| 08 事件与异步 | PASS | — |
| 09 端到端执行设计 | PASS | — |
| 10 状态机 | PASS | — |
| 11 算法与规则 | FAIL | A-11, 路由, merge_partial |
| 12 并发/取消/资源 | PARTIAL | epoch |
| 13 错误与恢复 | PASS | — |
| 14 前端与多端状态 | PASS | — |
| 15 安全与隐私设计 | PASS | — |
| 16 第三方与适配 | PARTIAL | 适配 |
| 17 性能预算 | PASS | — |
| 18 日志/指标/审计 | PARTIAL | 保留 |
| 19 配置与生命周期 | PARTIAL | 生效 |
| 20 迁移与兼容 | PARTIAL | 兼容矩阵 |
| 21 可测试性 | PASS | — |
| 22 评审与未决问题 | PASS | — |

## 第 4 步 · Agent 工作方式

| 条目 | 机器检查 | 缺失标记 |
|---|---|---|
| 01 范围与权威 | PASS | — |
| 02 工作包结构 | PASS | — |
| 03 角色与权限 | PASS | — |
| 04 任务卡字段 | PASS | — |
| 05 依赖类型 | PASS | — |
| 06 并行与冲突 | PASS | — |
| 07 调度与 WIP | PASS | — |
| 08 状态与进入退出 | PASS | — |
| 09 PR 拆分与分支 | PASS | — |
| 10 PR 内部流程 | PASS | — |
| 11 PR 描述与评审 | PASS | — |
| 12 CI 与合并权限 | PASS | — |
| 13 风险分级 | PASS | — |
| 14 分层验收 | PASS | — |
| 15 验收节点字段 | PASS | — |
| 16 发布与回退 | PASS | — |
| 17 人与 Agent 约定 | PASS | — |
| 18 阻塞与应急 | PASS | — |
| 19 进度与瓶颈 | PASS | — |
| 20 规范落实与例外 | PASS | — |

## 第 5 步 · 测试设计与验收规格

| 条目 | 机器检查 | 缺失标记 |
|---|---|---|
| 01 基线与对象 | PASS | — |
| 02 追踪矩阵 | PASS | — |
| 03 风险与不测 | PASS | — |
| 04 策略与责任 | PASS | — |
| 05 覆盖方法 | PASS | — |
| 06 判定依据 | PARTIAL | 禁止 |
| 07 用例规格 | PARTIAL | 编号与名称, 前置条件, 证据要求 |
| 08 执行与清理 | PASS | — |
| 09 环境与替身 | PARTIAL | 不能证明 |
| 10 数据与隔离 | PASS | — |
| 11 一致性与恢复 | PASS | — |
| 12 安全与权限 | PARTIAL | 越权 |
| 13 性能与成本 | PASS | — |
| 14 体验与多端 | PASS | — |
| 15 安装与升级 | PASS | — |
| 16 自动化与回归 | PARTIAL | flaky |
| 17 证据与报告 | FAIL | 分母, 五, 无法判定 |
| 18 缺陷管理 | PASS | — |
| 19 进入/通过/退出/发布 | PARTIAL | 阻断 |
| 20 角色与维护 | PASS | — |
| 21 AI-1 系统而非模型 | PASS | — |
| 22 AI-2 过程与禁止行为 | PARTIAL | 越权 |
| 23 AI-3 task/trial 口径 | FAIL | task, trial, n/N |
| 24 AI-4 评分器验证 | PASS | — |

**合计：83/103 PASS；PARTIAL 16；FAIL 4**
