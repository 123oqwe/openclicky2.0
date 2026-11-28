#!/usr/bin/env python3
"""Check each specification document against the review criteria.

The criteria are encoded as required markers per numbered item, taken from the
reference conversation (parts 1-6). This is a presence check, not a judgement:
a PASS means "the document addresses this item"; the human review record in
docs/reviews/ carries the verdict and the residual doubts.

A marker counts only inside the section it belongs to. Searching the whole
document let a term anywhere in it — a cross-reference, a passing mention in a
neighbouring section — satisfy an item whose own section said nothing about it,
which is the failure this check exists to catch. PARTIAL fails for the same
reason: a section carrying half its markers is a section nobody finished.

    doc-audit.py            render the table into docs/reviews/audit-output.md
    doc-audit.py --check    render in memory, fail if the file on disk differs
"""
import sys, pathlib, re

# A Windows console defaults to cp1252; printing Chinese would raise before any
# check ran. The scripts are the portable part, so they force their own encoding.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "docs" / "reviews" / "audit-output.md"

# `## 22.1 模型接入` is how 01-tech-prd splits section 22 and `### 06.1` is a
# subsection of SDD 06, so a sub-numbered heading opens the section of its own
# number. `(?![0-9])` keeps `## 123 x` from being read as section 12.
SECTION = re.compile(r"^#{2,3}\s+(?:(\d{1,2})(?:\.\d+)?)(?![0-9])")

def items(*pairs):
    return list(pairs)

STEP1 = items(
 ("01 基线/负责人/范围", ["文档编号", "适用发布", "已批准", "已验证", "裁决"]),
 ("02 背景/约束/假设/目标/非目标", ["硬约束", "待验证假设", "非目标", "方案选择"]),
 ("03 产品→技术追踪", ["派生自", "责任组件", "验证", "TR-"]),
 ("04 质量目标与技术保证", ["SLI", "p95", "INV-1", "统计口径"]),
 ("05 边界与总体架构", ["系统上下文", "部署视图", "数据流与信任边界", "箭头的含义"]),
 ("06 子系统划分与所有权", ["明确不负责", "拥有的权威数据", "失败影响", "所有权规则"]),
 ("07 流程/状态机/并发", ["状态机", "并发规则矩阵", "取消", "失败去向"]),
 ("08 数据架构与一致性", ["权威", "派生", "事务", "双写", "删除传播"]),
 ("09 接口与契约", ["前置条件", "成功含义", "失败分类", "超时", "重复调用"]),
 ("10 客户端与本地状态", ["离线", "崩溃", "休眠", "端侧"]),
 ("11 选型与证据", ["候选", "退出成本", "验证状态", "可行性"]),
 ("12 安全架构与隐私", ["资产", "威胁", "执行点", "撤销"]),
 ("13 故障模型", ["明确未执行", "结果未知", "重试", "补偿", "幂等"]),
 ("14 性能与容量", ["预算", "容量模型", "过载"]),
 ("15 可观测性", ["运行日志", "审计", "关联键", "告警", "禁止记录"]),
 ("16 环境与供应链", ["环境", "可重复", "供应链", "最小权限"]),
 ("17 迁移与升级", ["在途任务", "备份", "兼容", "回退"]),
 ("18 验证与准入", ["L1", "L6", "G3", "未验证项"]),
 ("19 成本与预算", ["单位", "日预算", "月预算", "估算", "假设"]),
 ("20 实施分解", ["并行条件", "集成顺序", "出口证据"]),
 ("21 风险/债/索引/变更", ["风险登记册", "技术债", "决策索引", "变更控制"]),
 ("22 AI/Agent/桌面专项", ["模型接入", "Harness", "结果核验", "质量评测", "继承", "端侧能力矩阵"]),
)

STEP2_FIELDS = [
 "01 标识", "02 决策问题", "03 背景", "04 决策驱动因素", "05 候选方案",
 "06 方案比较", "07 决策结论", "08 选择理由", "09 后果",
 "10 实施约束", "11 重新审视条件", "12 评审",
]

STEP3 = items(
 ("01 标识/版本/范围/状态", ["文档标识", "适用", "已批准"]),
 ("02 需求分配与约束", ["追踪", "不变量"]),
 ("03 结构分解与共用约定", ["workspace 布局", "共用约定"]),
 ("04 单元详细设计", ["前置条件", "后置条件", "副作用", "资源责任"]),
 ("05 数据模型与字典", ["DDL", "数据字典", "CREATE TABLE"]),
 ("06 存储与事务", ["事务边界", "写入路径"]),
 ("07 同步接口契约", ["成功含义", "超时", "重复"]),
 ("08 事件与异步", ["AppEvent", "顺序保证"]),
 ("09 端到端执行设计", ["流程 1", "流程 2", "流程 3"]),
 ("10 状态机", ["状态机", "转换"]),
 ("11 算法与规则", ["A-11", "路由", "merge_partial"]),
 ("12 并发/取消/资源", ["并发", "取消", "epoch"]),
 ("13 错误与恢复", ["错误", "重试", "补偿", "code"]),
 ("14 前端与多端状态", ["窗口", "本地状态", "端侧"]),
 ("15 安全与隐私设计", ["SEC-", "执行点"]),
 ("16 第三方与适配", ["适配", "降级"]),
 ("17 性能预算", ["预算", "并发上限"]),
 ("18 日志/指标/审计", ["事件名", "保留"]),
 ("19 配置与生命周期", ["config.toml", "生命周期", "生效"]),
 ("20 迁移与兼容", ["迁移", "兼容矩阵", "回退"]),
 ("21 可测试性", ["Clock", "假实现", "注入"]),
 ("22 评审与未决问题", ["未决问题", "变更记录"]),
)

STEP4 = items(
 ("01 范围与权威", ["权威来源", "冲突处理"]),
 ("02 工作包结构", ["工作包", "验收节点"]),
 ("03 角色与权限", ["角色", "集成负责人", "权限分离"]),
 ("04 任务卡字段", ["编号", "合并前置", "可修改范围"]),
 ("05 依赖类型", ["决策/规格", "合并依赖", "判定证据"]),
 ("06 并行与冲突", ["共享写区", "隔离"]),
 ("07 调度与 WIP", ["WIP", "里程碑"]),
 ("08 状态与进入退出", ["任务实施", "PR 评审", "功能验收", "发布"]),
 ("09 PR 拆分与分支", ["分支", "squash", "叠加 PR"]),
 ("10 PR 内部流程", ["接单", "建立验证", "自检", "交接"]),
 ("11 PR 描述与评审", ["模板", "没改什么", "交接"]),
 ("12 CI 与合并权限", ["merge_group", "必需检查", "例外"]),
 ("13 风险分级", ["高", "按影响"]),
 ("14 分层验收", ["G0", "G4"]),
 ("15 验收节点字段", ["版本基线", "失效", "解锁"]),
 ("16 发布与回退", ["观察", "回退", "收尾"]),
 ("17 人与 Agent 约定", ["允许自动", "需要人类", "证据"]),
 ("18 阻塞与应急", ["返工", "契约变化", "应急"]),
 ("19 进度与瓶颈", ["周期时间", "返工比例"]),
 ("20 规范落实与例外", ["分支保护", "例外"]),
)

STEP5 = items(
 ("01 基线与对象", ["被测对象", "非目标"]),
 ("02 追踪矩阵", ["TC-", "要求"]),
 ("03 风险与不测", ["优先级", "明确不测"]),
 ("04 策略与责任", ["单元", "集成", "手工"]),
 ("05 覆盖方法", ["等价类", "边界值", "状态迁移"]),
 ("06 判定依据", ["判定依据", "禁止"]),
 ("07 用例规格", ["编号与名称", "前置条件", "证据要求", "不能证明"]),
 ("08 执行与清理", ["初始化", "清理", "sleep"]),
 ("09 环境与替身", ["替身", "保真", "不能证明"]),
 ("10 数据与隔离", ["fixtures", "TempDir"]),
 ("11 一致性与恢复", ["事务", "恢复", "重放"]),
 ("12 安全与权限", ["沙箱", "SSRF", "越权"]),
 ("13 性能与成本", ["负载", "统计"]),
 ("14 体验与多端", ["渲染", "三平台"]),
 ("15 安装与升级", ["安装", "升级", "迁移"]),
 ("16 自动化与回归", ["flaky", "断言"]),
 ("17 证据与报告", ["分母", "五", "无法判定"]),
 ("18 缺陷管理", ["复现", "回归"]),
 ("19 进入/通过/退出/发布", ["进入", "退出", "阻断"]),
 ("20 角色与维护", ["维护人", "变更"]),
 ("21 AI-1 系统而非模型", ["提示词", "请求体", "状态"]),
 ("22 AI-2 过程与禁止行为", ["禁止行为", "越权"]),
 ("23 AI-3 task/trial 口径", ["task", "trial", "n/N"]),
 ("24 AI-4 评分器验证", ["评分", "反例"]),
)

def sections(text):
    """Body of every numbered section, keyed by the number on its heading."""
    found = {}
    current = None
    body = []
    for line in text.splitlines():
        m = SECTION.match(line)
        if m:
            number = int(m.group(1)) if m.group(1) else None
            if number != current:
                if current is not None and current not in found:
                    found[current] = "\n".join(body)
                current = number
                body = []
        if current is not None:
            body.append(line)
    if current is not None and current not in found:
        found[current] = "\n".join(body)
    return found

def section_of(name):
    return int(re.match(r"^(\d{1,2})", name).group(1))

def check_file(path, checks):
    bodies = sections(path.read_text(encoding="utf-8").lower())
    rows = []
    for name, markers in checks:
        body = bodies.get(section_of(name))
        if body is None:
            rows.append((name, "FAIL", ["（无此编号章节）"]))
            continue
        missing = [m for m in markers if m.lower() not in body]
        status = "PASS" if not missing else ("PARTIAL" if len(missing) < len(markers) else "FAIL")
        rows.append((name, status, missing))
    return rows

def check_adrs():
    adr_dir = ROOT / "docs" / "adr"
    files = sorted(p for p in adr_dir.glob("ADR-*.md"))
    rows = []
    for f in files:
        text = f.read_text(encoding="utf-8")
        numbered = set()
        for line in text.splitlines():
            if line.startswith("## "):
                numbered.add(line.strip()[3:5])
            elif line.startswith("| ") and len(line) > 4 and line[2:4].isdigit():
                numbered.add(line[2:4])
        missing = [fl for fl in STEP2_FIELDS if fl.split()[0] not in numbered]
        extra = []
        for need in ["决策者", "实施状态", "中性变化", "符合决策"]:
            if need not in text:
                extra.append(need)
        status = "PASS" if not missing and not extra else ("PARTIAL" if not missing else "FAIL")
        rows.append((f.name, status, missing + extra))
    return rows

def fmt(title, rows):
    out = [f"## {title}", "", "| 条目 | 机器检查 | 缺失标记 |", "|---|---|---|"]
    for name, status, missing in rows:
        out.append(f"| {name} | {status} | {', '.join(missing) if missing else '—'} |")
    out.append("")
    return "\n".join(out)

def main(argv):
    adr_rows = check_adrs()
    steps = [
        ("第 1 步 · 技术实现 PRD", "docs/01-tech-prd.md", check_file(ROOT/"docs"/"01-tech-prd.md", STEP1)),
        (f"第 2 步 · ADR（{len(adr_rows)} 份）", "docs/adr", adr_rows),
        ("第 3 步 · SDD", "docs/02-sdd.md", check_file(ROOT/"docs"/"02-sdd.md", STEP3)),
        ("第 4 步 · Agent 工作方式", "docs/03-agent-playbook.md", check_file(ROOT/"docs"/"03-agent-playbook.md", STEP4)),
        ("第 5 步 · 测试设计与验收规格", "docs/04-test-design.md", check_file(ROOT/"docs"/"04-test-design.md", STEP5)),
    ]
    findings = []
    statuses = {}
    parts = ["# 文档验收：机器检查结果\n"]
    for title, where, rows in steps:
        parts.append(fmt(title, rows))
        for name, status, missing in rows:
            statuses[status] = statuses.get(status, 0) + 1
            if status != "PASS":
                findings.append(
                    f"VIOLATION: {where} · {name} · {status} · missing={', '.join(missing)}"
                )
    passed = statuses.get("PASS", 0)
    total = sum(statuses.values())
    parts.append(
        "**合计：{}/{} PASS；PARTIAL {}；FAIL {}**\n".format(
            passed, total, statuses.get("PARTIAL", 0), statuses.get("FAIL", 0)
        )
    )
    document = "\n".join(parts)

    if "--check" in argv:
        on_disk = OUTPUT.read_text(encoding="utf-8") if OUTPUT.exists() else None
        # PARTIAL/FAIL rows are printed for the debt ratchet. This mode fails
        # only when the committed table is not what the documents render to.
        stale = on_disk != document
        if stale:
            findings.append(
                f"VIOLATION: {OUTPUT.relative_to(ROOT).as_posix()} does not match what the "
                "documents render to; run python3 scripts/doc-audit.py and commit the result"
            )
        for line in findings:
            print(line)
        print(f"doc-audit: {passed}/{total} PASS")
        return 1 if stale else 0

    OUTPUT.write_text(document, encoding="utf-8")
    print(document)
    for line in findings:
        print(line)
    print(f"doc-audit: {passed}/{total} PASS")
    return 1 if findings else 0

if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
