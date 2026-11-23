#!/usr/bin/env python3
"""Numbered-item coverage: does each document contain every item the reference
conversation requires?

Inputs are the verbatim requirement lists in
docs/reviews/standards/source-conversation.md. This script only checks that each
numbered item has a home in the document; `scripts/doc-audit.py` checks the
substance markers, and docs/reviews/step*.md carry the human verdict.
"""
import re, sys, pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent

PART1 = [
 "文档基本信息与生效范围","产品摘要与产品定义","用户问题、研究证据与立项依据","目标用户、角色与利益相关方",
 "核心场景、用户任务与需求","产品目标与成功指标","产品定位、设计原则与方案取舍","本次范围、非目标、优先级与版本边界",
 "产品能力结构与信息架构","核心业务对象、术语与关系","端到端用户旅程与业务流程","逐项功能需求与统一功能规格",
 "业务规则与跨模块一致性","状态、异常、边界与恢复","交互、界面、内容与通知","权限、安全与数据生命周期",
 "非功能需求与质量标准","外部集成、兼容、迁移与退出","商业规则、运营与支持能力","AI 能力边界、控制权与结果可信度",
 "产品分析、反馈与假设验证","产品验收标准与需求追踪","发布范围、阶段目标与上线后处理","风险、假设、待决策事项与变更记录",
]
PART2 = [
 "文档基线、负责人和适用范围","技术背景、现状、目标与非目标","产品需求到技术需求的追踪关系","可衡量的质量目标与技术保证",
 "系统边界与总体架构","子系统划分、职责与所有权","关键运行流程、状态机与并发规则","数据架构、权威来源与一致性",
 "接口、事件与跨模块契约","客户端、多端协作与本地状态","技术选型、外部依赖与可行性证据","安全架构、权限执行与隐私保护",
 "故障模型、重试、恢复与外部副作用","性能、容量与资源隔离","可观测性、审计与运行支持","环境、构建、部署与软件供应链",
 "数据迁移、版本兼容、升级与回退","验证策略、证据要求与发布准入","成本模型与预算控制","实施分解、依赖关系与集成顺序",
 "风险、技术债、决策索引与变更控制","AI、Agent、平台和操作系统专项要求",
]
PART3 = [
 "决策标识、状态、负责人和适用范围","决策问题与范围边界","背景、约束、假设与证据","决策驱动因素与优先顺序",
 "真正考虑过的候选方案","方案比较与证据强度","清晰、可执行的决策结论","选择理由与关键取舍",
 "正面后果、负面后果与剩余风险","实施约束与落实确认","重新审视条件、可逆性与退出路径","评审、异议、关联决策与历史",
]
PART4 = [
 "文档标识、版本、负责人和适用范围","需求分配、设计约束与追踪关系","系统位置、结构分解与共用约定","模块、组件与关键软件单元的详细设计",
 "数据模型、数据结构与数据字典","存储、事务、读写路径与一致性","同步接口与调用契约","事件、消息和异步协议",
 "关键场景的端到端执行设计","状态机与状态转换规则","算法、业务规则和关键计算","并发、调度、取消与资源协调",
 "错误分类、重试、恢复与补偿","前端、交互实现与多端状态","安全控制、授权执行与隐私设计","第三方系统、插件与复用组件",
 "性能预算、容量和资源控制","日志、指标、调用追踪与审计","配置、部署与运行生命周期","迁移、版本兼容与回退",
 "可测试性设计与验证接口","设计评审、未决问题与变更记录",
]
PART5 = [
 "文档基线、测试对象与目标","测试依据与双向追踪矩阵","风险分析、优先级与不测范围","测试策略、层级与责任分配",
 "场景模型、测试条件与覆盖方法","正确性判定依据","单个测试用例的完整规格","执行规程、初始化与清理",
 "测试环境、替身范围与真实性边界","测试数据、版本与隔离","数据一致性、并发、故障与恢复","安全、权限与隐私测试",
 "性能、容量、稳定性与成本","用户体验、多端兼容与无障碍","安装、升级、迁移与运行验收","自动化策略、回归与测试自身质量",
 "执行记录、证据包与结果报告","缺陷管理、修复确认与回归","进入条件、通过条件、退出条件与发布规则","角色、资源、变更与维护规则",
]
PART5_AI = [
 "测整个系统而不只是模型","结果、过程约束与禁止行为","数据集、重复运行与统计口径","评分器也需要验证",
]
PART6 = [
 "文档范围、适用版本与权威来源","交付物分解与工作包结构","角色、所有权与决策权限","统一的任务卡与 PR 计划字段",
 "带类型、条件和证据的依赖关系","并行开发条件与资源冲突规则","调度、工作量、在制品限制与里程碑","状态定义与进入、退出条件",
 "PR 拆分原则与分支策略","单个 PR 的标准执行流程","PR 描述、评审与交接要求","CI、合并队列与合并权限",
 "风险分级与专项检查触发条件","分层验收","每个验收节点的完整定义","发布、观察、回退与收尾",
 "人与 Agent 的执行约定","阻塞、返工、依赖变更与应急路径","执行进度、瓶颈与工程反馈","规范落实、更新与例外管理",
]

def headings(text):
    return [l.strip() for l in text.splitlines() if re.match(r'^#{2,3}\s', l.strip())]

def numbered_headings(text, limit):
    found = {}
    for h in headings(text):
        m = re.match(r'^#{2,3}\s+(\d{1,2})[\.、 ]', h)
        if m:
            n = int(m.group(1))
            if 1 <= n <= limit and n not in found:
                found[n] = h
    return found

def report(title, path, items, extra_marker=None):
    text = (ROOT / path).read_text(encoding='utf-8')
    found = numbered_headings(text, len(items))
    rows = []
    ok = 0
    for i, name in enumerate(items, 1):
        h = found.get(i)
        status = "OK" if h else "MISSING"
        if h:
            ok += 1
        rows.append((i, name, h or "—", status))
    out = [f"## {title}", "", f"文件：`{path}`　条目：{ok}/{len(items)}", "",
           "| # | 标准条目（原文） | 文档中的位置 | 结论 |", "|---|---|---|---|"]
    for i, name, h, st in rows:
        out.append(f"| {i:02d} | {name} | {h} | {st} |")
    if extra_marker:
        present = extra_marker in text
        out.append("")
        out.append(f"判断标准小节：{'存在' if present else '**缺失**'}")
    out.append("")
    return "\n".join(out), ok, len(items)

def main():
    parts = [
        report("第 2 部分 · 技术实现 PRD", "docs/01-tech-prd.md", PART2, "判断标准"),
        report("第 4 部分 · SDD", "docs/02-sdd.md", PART4, "设计评审"),
        report("第 6 部分 · Agent 工作方式", "docs/03-agent-playbook.md", PART6, "最终审查"),
        report("第 5 部分 · 测试设计与验收规格", "docs/04-test-design.md", PART5 + PART5_AI, "判据自检"),
    ]
    # ADRs: 12 fields across every file
    adr_dir = ROOT / "docs" / "adr"
    files = sorted(adr_dir.glob("ADR-*.md"))
    adr_rows = []
    all_ok = True
    for f in files:
        text = f.read_text(encoding='utf-8')
        numbered = set()
        for line in text.splitlines():
            if line.startswith("## "):
                m = re.match(r'^##\s+(\d{1,2})\b', line)
                if m:
                    numbered.add(int(m.group(1)))
            elif line.startswith("| ") and len(line) > 4:
                head = line[2:].split("|")[0].strip()
                m = re.match(r'^(\d{1,2})\b', head)
                if m:
                    numbered.add(int(m.group(1)))
        missing = [i for i in range(1, 13) if i not in numbered]
        if missing:
            all_ok = False
        adr_rows.append((f.name, "OK" if not missing else f"缺 {missing}"))
    adr_out = ["## 第 3 部分 · ADR（12 个字段 × 13 份）", "",
               "| 文件 | 12 字段 |", "|---|---|"]
    for name, st in adr_rows:
        adr_out.append(f"| {name} | {st} |")
    adr_out.append("")

    doc = ["# 标准条目覆盖矩阵", "",
           "本矩阵逐条回答：对话里要求的每一项，在文档里有没有对应的位置。",
           "（内容是否合格由 `scripts/doc-audit.py` 与 `docs/reviews/step*.md` 判断。）", ""]
    total_ok = 0
    total = 0
    for text, ok, n in parts:
        doc.append(text)
        total_ok += ok
        total += n
    doc.append("\n".join(adr_out))
    doc.append(f"**合计：{total_ok}/{total} 项有位置；ADR 12 字段：{'全部齐全' if all_ok else '有缺失'}**")
    (ROOT / "docs/reviews/standards/coverage.md").write_text("\n".join(doc), encoding="utf-8")
    print("\n".join(doc[-3:]))
    return 0 if all_ok and total_ok == total else 1

if __name__ == "__main__":
    sys.exit(main())
