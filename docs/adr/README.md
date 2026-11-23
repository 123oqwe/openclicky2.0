# 架构决策记录（ADR）

| 编号 | 标题 | 状态 |
|---|---|---|
| [ADR-0001](ADR-0001-desktop-stack.md) | 桌面技术栈：Tauri 2 + Rust 核心 + 无框架 TypeScript | Accepted |
| [ADR-0002](ADR-0002-local-first.md) | 本地优先单进程架构，不设自有服务端 | Accepted |
| [ADR-0003](ADR-0003-screen-capture.md) | 屏幕采集：统一抽象 + 硬门禁，只在按键瞬间采集 | Accepted |
| [ADR-0004](ADR-0004-input-injection.md) | 输入注入：enigo + 剪贴板兜底 + 三重守卫 | Accepted |
| [ADR-0005](ADR-0005-model-access.md) | 模型访问：自研薄 provider 层，同步 HTTP，多供应商可切 | Accepted |
| [ADR-0006](ADR-0006-annotation-protocol.md) | 指与画：行内标签注解协议 + 像素坐标 + 显式换算 | Accepted |
| [ADR-0007](ADR-0007-agent-runtime.md) | Agent 运行时：单 Agent 多任务，模型外策略层，检查点可恢复 | Accepted |
| [ADR-0008](ADR-0008-persistence.md) | 持久化：SQLite + Markdown 记忆 + TOML 配置，版本化迁移 | Accepted |
| [ADR-0009](ADR-0009-secrets-privacy.md) | 密钥与隐私：钥匙串 + 全链路脱敏 + 可证明的采集门禁 | Accepted |
| [ADR-0010](ADR-0010-usage-budget.md) | 用量：以"预算护栏"替代商业计费 | Accepted |
| [ADR-0011](ADR-0011-distribution.md) | 三平台交付：CI 矩阵，默认未签名可用 | Accepted |
| [ADR-0012](ADR-0012-testing.md) | 测试与验证：端口注入 + 假 provider，离线确定性 | Accepted |
| [ADR-0013](ADR-0013-dictation-streaming.md) | 听写：增量重转写 + 最长公共前缀合并 | Accepted |

生命周期：`Proposed → Accepted → (Superseded by ADR-xxxx | Deprecated)`。决策改变时**新增** ADR 并把旧的标为 Superseded，不改写历史。
