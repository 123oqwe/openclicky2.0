# 验收记录（每一步一个文件）

规则：**上一步没有通过验收，不开始下一步。**

| 步 | 文档 | 验收记录 | 标准来源 | 结论 |
|---|---|---|---|---|
| 1 | `docs/01-tech-prd.md` | `step1-tech-prd.md` | 对话·第二部分（22 项 + 判断标准） | PASS |
| 2 | `docs/adr/ADR-*.md` | `step2-adr.md` | 对话·第三部分（12 项 + 9 条检查） | PASS |
| 3 | `docs/02-sdd.md` | `step3-sdd.md` | 对话·第四部分（22 项 + 10 条检查） | PASS |
| 4 | `docs/03-agent-playbook.md` + `AGENTS.md` | `step4-playbook.md` | 对话·第六部分（01–20） | PASS |
| 5 | `docs/04-test-design.md` | `step5-tds.md` | 对话·第五部分（20 项 + AI 4 条） | PASS |
| 6 | 代码/CI | `step6-implementation.md`（待写） | 第二至五部分的下游约束 | 冻结中 |

## 标准出处

- 验收标准原文：`docs/reviews/standards/source-conversation.md`（从共享链接提取，未改写）。
- 条目覆盖矩阵：`docs/reviews/standards/coverage.md`（`scripts/criteria-coverage.py` 生成，`--check` 校验是否过期）。
- 产品 PRD 逐条对照与缺口：`docs/reviews/standards/product-prd-gap.md`。

## 判定口径

- **机器检查**（`scripts/doc-audit.py`）：只回答"这一项被写到了吗"，输出 `docs/reviews/audit-output.md`；标记只在本条目自己的章节里找。PASS 不代表内容合格。
- **人工判定**：回答"写到了且写对了没有"，逐条给证据位置与残留疑问。
- 结论只有三种：`PASS`（可进入下一步）、`PASS WITH RISK`（可进入，但风险已登记）、`FAIL`（必须修完再验）。

## 过程偏差记录（诚实披露）

1. 初版技术实现 PRD（v1.0）密度不足，且我在未完成其深度自查时就写了 ADR/SDD。已通过 v2.0 重写 + 本目录逐项补证纠正；ADR/SDD/TDS 均在 v2 文档基础上复核。
2. 代码在文档链验收前已被写出（`feat/core` 分支，提交 `4a99bd8`）。已**冻结**：不入主干、不继续开发，直到第 5 步验收通过。第 6 步将重新以验收后的文档为准检查代码。
