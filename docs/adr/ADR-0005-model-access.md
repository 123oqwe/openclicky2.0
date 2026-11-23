# ADR-0005 自研薄 provider 层，不引入官方 SDK 或聚合框架

| 字段 | 内容 |
|---|---|
| 01 标识 | ADR-0005 |
| 状态 | **Accepted**（决策已接受，2026-11-23） |
| 决策者 | 人类维护者（本项目唯一有权接受者） |
| 实施状态 | 未实现（本 ADR 为设计基线；实现进度见关联 PR 与 `06-acceptance.md`） |
| 负责人 | 技术负责人 |
| 适用范围 | 对话、转写、语音合成、模型探测 |
| 关联 | OC2-TECH-PRD §09、§22.1、§19；ADR-0012 |

## 02 决策问题与范围边界

用官方 SDK 还是自研 HTTP 客户端？同步还是 async？支持哪些协议？边界：不做模型微调、不做本地推理。

## 03 背景、约束、假设与证据

- 需要三种能力：视觉对话（流式）、语音转写（multipart）、语音合成（二进制）。
- 需要可替换供应商（C-6），且要能指向"本地 OpenAI 兼容服务"（LM Studio / whisper.cpp server）。
- 测试必须离线（C-5）→ HTTP 必须可注入。
- 证据：三类能力在 OpenAI 兼容协议下形态稳定；Anthropic messages 协议差异仅在消息结构与图片编码。

## 04 决策驱动因素

1. 可测性（离线、确定性）
2. 供应商可替换 + 本地端点
3. 依赖重量与安全面
4. 流式体验

## 05 候选方案

| 方案 | 说明 |
|---|---|
| A. 自研 provider 层 + `ureq` 同步 + trait 注入 | 手写协议 |
| B. 官方 SDK（async reqwest + tokio） | 供应商各自 SDK |
| C. `rig`/`llm` 等聚合框架 | 第三方抽象 |

## 06 方案比较与证据强度

| 维度 | A | B | C |
|---|---|---|---|
| 离线可测 | 强（trait 假实现） | 中（要 httpmock） | 中 |
| 依赖重量 | 轻 | 重（tokio+多 SDK） | 重 |
| 本地兼容端点 | 强 | 中 | 弱 |
| 协议漂移风险 | 中（自己维护） | 低 | 中 |

证据强度：A **强**（架构可判定）；B/C **中**。

## 07 决策结论

采用 **A**：

- `providers::{openai, anthropic}`：仅实现用到的最小端点（chat/stream、transcriptions、speech、models）。
- 传输：`HttpTransport` trait；生产 `UreqTransport`；测试 `ScriptedTransport`（按请求序号返回预置响应或错误）。
- 流式：SSE 按行解析，`[DONE]` 结束；解析失败降级整段响应。
- 错误分类：`Auth / RateLimit / RegionBlocked / Outage / Timeout / BadRequest / Protocol`，映射为用户可读中文文案。
- 重试：仅 `RateLimited/Outage/Timeout/Network` 且**流未产出内容**时；最多 2 次重试（共 3 次尝试），500ms/1500ms。与 tech-prd §13.2 同一集合。

## 08 理由与取舍

取舍：自己维护协议适配（约 400 行）换取零 async 运行时与完全可测；放弃 SDK 的自动重试/遥测。

## 09 后果与剩余风险

中性变化：职责与接口按本决定重新分配，团队/Agent 的工作方式随之调整；既有习惯做法需要迁移。

正面：`cargo test` 无需网络；换供应商只加一个文件；无遥测。

负面：新协议特性（如原生 JSON schema 输出）需自己补；供应商破坏性变更需跟进。

剩余风险：无可用密钥 → 在线路径未实测（R-2），以 `doctor --online` 由用户补齐。

## 10 实施约束与落实确认

- `core` 不得直接依赖 `ureq`；只依赖 `HttpTransport`。
- 所有出站请求必须带超时与 User-Agent。
- 落实确认：`tests/providers_openai.rs`、`tests/providers_anthropic.rs`、`tests/sse_parse.rs`。

落实确认分两类：**符合决策**（实现是否遵守本决定的约束）与**达到预期**（遵守之后是否真的改善了当初关注的问题）。前者通过不证明后者成立；若仅前者成立，应重新评估本决定而不是宣布成功。

## 11 重审条件、可逆性、退出路径

重审：需要多模态输出（音频/图）、原生工具调用 schema、或供应商要求 OAuth。

可逆性：高。退出：替换 `UreqTransport` 为 reqwest，或整体换 SDK（provider 接口不变）。

## 12 评审、异议、关联与历史

关联 ADR-0002（BYO key）、ADR-0012（测试）。历史：2026-11-23 创建并接受。
