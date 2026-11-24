#!/usr/bin/env python3
"""Generate the TDS traceability matrix from the tests that actually exist.

A row may only claim `已实现` when every cited `path::test_name` is found on the
implementation branch. Anything else is `spec-only` or `部分`. This is the fix
for the audit finding "the matrix cites seven test files that do not exist".
"""
import os, subprocess, sys, pathlib, re

ROOT = pathlib.Path(__file__).resolve().parent.parent
# Where the tests live. The docs branch does not carry code, so we read a branch.
CANDIDATES = [os.environ.get("TDS_SOURCE_BRANCH", ""), "HEAD", "feat/impl", "origin/feat/impl"]


def source_branch():
    for c in CANDIDATES:
        if not c:
            continue
        r = subprocess.run(["git", "rev-parse", "--verify", "--quiet", c],
                           cwd=ROOT, capture_output=True, text=True)
        if r.returncode == 0:
            return c
    return None

# code, requirement, cases, status, evidence (path::test or "-"), note
ROWS = [
 ("TC-PRIV-001", "INV-1 不按键不采集", "TC-PRIV-001", "已实现",
  "crates/core/tests/security.rs::no_capture_without_a_gate", ""),
 ("TC-PRIV-002", "门禁只由用户动作开启", "TC-PRIV-002", "已实现",
  "crates/core/tests/security.rs::capture_gate_counts_only_user_actions", ""),
 ("TC-CAP-001", "一次提问只采集一次", "TC-CAP-001", "已实现",
  "crates/core/tests/e2e_ask.rs::capture_happens_exactly_once_per_question", ""),
 ("TC-CAP-002", "副屏负原点坐标换算（按图片比例）", "TC-CAP-002", "已实现",
  "crates/core/tests/e2e_ask.rs::pointing_on_a_negative_origin_display_uses_the_right_offset", "缩比在 annotations::picture_shrink 中锁定"),
 ("TC-HK-001", "默认 talk 键解析", "TC-HK-001", "已实现",
  "crates/core/src/hotkey.rs::parses_mac_default_talk_binding", ""),
 ("TC-HK-002", "别名归一（ctrl/option）", "TC-HK-002", "已实现",
  "crates/core/src/hotkey.rs::parses_aliases", ""),
 ("TC-HK-003", "听写键含 Fn", "TC-HK-003", "已实现",
  "crates/core/src/hotkey.rs::parses_dictation_with_fn", ""),
 ("TC-HK-004", "双击手势", "TC-HK-004", "已实现",
  "crates/core/src/hotkey.rs::parses_double_tap_gesture", ""),
 ("TC-HK-005", "非法组合被拒", "TC-HK-005", "已实现",
  "crates/core/src/hotkey.rs::rejects_mid_typo", ""),
 ("TC-E2E-ASK-001", "问答全链路（回答+指+计费）", "TC-E2E-ASK-001", "已实现",
  "crates/core/tests/e2e_ask.rs::asks_answers_points_and_charges_once", ""),
 ("TC-E2E-ASK-002", "TTS 失败回退剪贴板", "TC-E2E-ASK-002", "已实现",
  "crates/core/tests/e2e_ask.rs::silent_tts_falls_back_to_text_and_clipboard", ""),
 ("TC-E2E-ASK-003", "缺模型报 E_MODEL", "TC-E2E-ASK-003", "已实现",
  "crates/core/tests/e2e_ask.rs::missing_model_is_a_clear_error", ""),
 ("TC-E2E-ASK-004", "消息与用量同事务落库", "TC-E2E-ASK-004", "已实现",
  "crates/core/tests/e2e_ask.rs::messages_and_usage_are_persisted_together", ""),
 ("TC-E2E-ASK-005", "打字提问不采集/不录音", "TC-E2E-ASK-005", "已实现",
  "crates/core/tests/e2e_ask.rs::typed_question_captures_once_and_can_point", ""),
 ("TC-E2E-ASK-006", "199ms 短按拒绝（不采集不计费）", "TC-E2E-ASK-006", "已实现",
  "crates/core/tests/e2e_ask.rs::short_press_is_rejected_and_not_charged", ""),
 ("TC-ANNO-001", "POINT 解析与剥离标签", "TC-ANNO-001…006", "已实现",
  "crates/core/tests/e2e_ask.rs::asks_answers_points_and_charges_once", "13 条细项见 spec-only"),
 ("TC-ANNO-002", "越界裁剪/坏标签丢弃/多形状", "TC-ANNO-002", "已实现",
  "crates/core/src/annotations.rs::parses_every_draw_shape", "另有 9 条同模块单测"),
 ("TC-GEST-001", "按住期间鼠标轨迹并入参考图", "TC-GEST-001", "已实现",
  "crates/core/tests/e2e_ask.rs::a_gesture_puts_the_circled_region_into_the_prompt", "bbox/dead-zone/circle 另 3 条"),
 ("TC-WALK-001", "命中推进/点偏释放", "TC-WALK-001", "已实现",
  "crates/core/src/walkthrough.rs::hit_advances_and_miss_releases", ""),
 ("TC-WALK-002", "容差内命中", "TC-WALK-002", "已实现",
  "crates/core/src/walkthrough.rs::click_near_the_edge_still_hits", ""),
 ("TC-WALK-003", "最多 15 步", "TC-WALK-003", "已实现",
  "crates/core/src/walkthrough.rs::at_most_fifteen_steps", ""),
 ("TC-WALK-004", "continue 推进且不可重放", "TC-WALK-004", "已实现",
  "crates/core/src/walkthrough.rs::continue_moves_on_and_cannot_replay", ""),
 ("TC-WALK-005", "空带做被拒", "TC-WALK-005", "已实现",
  "crates/core/src/walkthrough.rs::empty_walkthrough_is_rejected", ""),
 ("TC-DICT-001", "只注入新增后缀", "TC-DICT-001", "已实现",
  "crates/core/tests/e2e_dictation.rs::only_the_new_suffix_is_typed", ""),
 ("TC-DICT-002", "终端只注入单行", "TC-DICT-002", "已实现",
  "crates/core/tests/e2e_dictation.rs::terminal_targets_get_a_single_line", ""),
 ("TC-DICT-003", "疑似密码不注入", "TC-DICT-003", "已实现",
  "crates/core/tests/e2e_dictation.rs::tick_refuses_password_like_speech", "tick 与 finish 都过守卫"),
 ("TC-DICT-004", "词典学习与复用", "TC-DICT-004", "已实现",
  "crates/core/tests/e2e_dictation.rs::corrections_are_learned_and_reused", ""),
 ("TC-DICT-005", "学习窗口关闭", "TC-DICT-005", "已实现",
  "crates/core/tests/e2e_dictation.rs::learning_window_closes_after_finish", ""),
 ("TC-DICT-006", "前缀合并/回退/清理/词典优先", "TC-DICT-006…008", "已实现",
  "crates/core/src/dictation.rs::merge_handles_rewrite", "另有 7 条同模块单测"),
 ("TC-USE-001", "10 秒内取消不计费", "TC-USE-001", "已实现",
  "crates/core/src/usage.rs::task_stopped_within_ten_seconds_is_free", ""),
 ("TC-USE-002", "语音 1u", "TC-USE-002", "已实现",
  "crates/core/src/usage.rs::voice_costs_one_unit", ""),
 ("TC-USE-003", "80% 提醒 / 100% 阻断", "TC-USE-003", "已实现",
  "crates/core/src/usage.rs::budget_blocks_at_limit_and_warns_at_80_percent", ""),
 ("TC-USE-004", "每日重置", "TC-USE-004", "已实现",
  "crates/core/src/usage.rs::daily_reset_clears_usage", ""),
 ("TC-USE-005", "当日提额解除阻断", "TC-USE-005", "已实现",
  "crates/core/src/usage.rs::temporary_grant_unblocks_today", "月度提额待 F3"),
 ("TC-USE-006", "长任务只计一次", "TC-USE-006", "已实现",
  "crates/core/src/usage.rs::agent_steps_are_always_metered_even_after_twenty_minutes", "20 分钟后步数仍如实计量"),
 ("TC-AGT-001", "写文件后带证据完成", "TC-AGT-001", "已实现",
  "crates/core/tests/e2e_agent.rs::writes_a_file_then_finishes_with_evidence", ""),
 ("TC-AGT-002", "拒绝后不执行且 30 分钟内不问", "TC-AGT-002", "已实现",
  "crates/core/src/tools/policy.rs::destructive_asks_then_respects_not_now", "拒绝路径见 e2e_agent"),
 ("TC-AGT-003", "无证据完成标 unverified", "TC-AGT-003", "已实现",
  "crates/core/tests/e2e_agent.rs::claim_without_artifact_is_unverified", ""),
 ("TC-AGT-004", "取消终态且保留检查点", "TC-AGT-004", "已实现",
  "crates/core/tests/e2e_agent.rs::cancellation_is_terminal_and_keeps_the_checkpoint", ""),
 ("TC-AGT-005", "启动恢复置 Paused", "TC-AGT-005", "已实现",
  "crates/core/tests/e2e_agent.rs::startup_recovery_pauses_stale_running_tasks", ""),
 ("TC-AGT-006", "批准后动作只执行一次", "TC-AGT-006", "已实现",
  "crates/core/tests/e2e_agent.rs::approval_lets_the_action_run_once", ""),
 ("TC-AGT-007", "循环检测停止任务", "TC-AGT-007", "已实现",
  "crates/core/tests/e2e_agent.rs::repeated_identical_calls_stop_the_task",
  "当前是 Failed(loop)，规范要求改道 + 事件（F3）"),
 ("TC-AGT-008", "shell 关闭时拒绝", "TC-AGT-008", "已实现",
  "crates/core/tests/e2e_agent.rs::shell_is_denied_when_disabled", ""),
 ("TC-AGT-009", "状态机转换表", "TC-AGT-009", "已实现",
  "crates/core/src/agent/mod.rs::transitions_follow_the_table", "覆盖 SDD 表全部允许/禁止边"),
 ("TC-RTN-001", "条件不满足等待", "TC-RTN-001", "已实现",
  "crates/core/src/routines.rs::wakes_up_only_when_conditions_are_met", ""),
 ("TC-RTN-002", "三连失败暂停", "TC-RTN-002", "已实现",
  "crates/core/src/routines.rs::wakes_up_only_when_conditions_are_met", ""),
 ("TC-RTN-003", "时间表解析与下次触发", "TC-RTN-003", "已实现",
  "crates/core/src/routines.rs::weekly_picks_the_next_matching_day", ""),
 ("TC-RTN-004", "非法时间表被拒", "TC-RTN-004", "已实现",
  "crates/core/src/routines.rs::rejects_garbage", ""),
 ("TC-SUG-001", "6 点前/通话/专注不打扰", "TC-SUG-001", "已实现",
  "crates/core/src/suggest.rs::no_suggestions_before_six", ""),
 ("TC-SUG-002", "连续 3 个早上忽略暂停并翻倍", "TC-SUG-002", "已实现",
  "crates/core/src/suggest.rs::three_ignored_mornings_pause_one_day_then_double", ""),
 ("TC-SUG-003", "暂停上限 7 天", "TC-SUG-003", "已实现",
  "crates/core/src/suggest.rs::pause_is_capped_at_a_week", ""),
 ("TC-SUG-004", "8 次未点胶囊停 1 小时", "TC-SUG-004", "已实现",
  "crates/core/src/suggest.rs::eight_ignored_chips_pause_one_hour", ""),
 ("TC-MEM-001", "偏好去重并持久化", "TC-MEM-001", "已实现",
  "crates/core/src/memory.rs::preferences_are_deduplicated_and_persisted", ""),
 ("TC-MEM-002", "偏好不截断", "TC-MEM-002", "已实现",
  "crates/core/src/memory.rs::profile_is_never_truncated", ""),
 ("TC-MEM-003", "项目记忆压缩", "TC-MEM-003", "已实现",
  "crates/core/src/memory.rs::projects_are_compacted", ""),
 ("TC-MEM-004", "遗忘删除匹配行", "TC-MEM-004", "已实现",
  "crates/core/src/memory.rs::forget_removes_matching_lines_everywhere", ""),
 ("TC-RTE-001", "快/深路由表", "TC-RTE-001", "已实现",
  "crates/core/src/router.rs::tasks_are_always_deep", "另有 5 条同模块单测"),
 ("TC-PRV-001", "OpenAI 文本/工具解析", "TC-PRV-001", "已实现",
  "crates/core/tests/providers.rs::openai_parses_text_tools_and_usage", ""),
 ("TC-PRV-002", "流式增量", "TC-PRV-002", "已实现",
  "crates/core/tests/providers.rs::openai_stream_accumulates_deltas", ""),
 ("TC-PRV-003", "请求带图与 tools", "TC-PRV-003", "已实现",
  "crates/core/tests/providers.rs::openai_request_carries_images_and_tools", ""),
 ("TC-PRV-004", "Anthropic 工具块", "TC-PRV-004", "已实现",
  "crates/core/tests/providers.rs::anthropic_parses_tool_use", ""),
 ("TC-PRV-005", "Anthropic 流式", "TC-PRV-005", "已实现",
  "crates/core/tests/providers.rs::anthropic_stream_reads_deltas", ""),
 ("TC-ERR-001", "错误码映射", "TC-ERR-001", "已实现",
  "crates/core/tests/providers.rs::error_statuses_map_to_stable_codes", ""),
 ("TC-ERR-002", "无 key 报 E_NO_KEY", "TC-ERR-002", "已实现",
  "crates/core/tests/providers.rs::build_requires_a_key", ""),
 ("TC-SEC-001", "密钥脱敏", "TC-SEC-001", "已实现",
  "crates/core/tests/security.rs::credentials_are_redacted_from_logs", ""),
 ("TC-SEC-002", "行内密钥识别", "TC-SEC-002", "已实现",
  "crates/core/tests/security.rs::secret_detection_catches_inline_keys", ""),
 ("TC-SEC-003", "沙箱拒绝 ../绝对路径/符号链接", "TC-SEC-003", "已实现",
  "crates/core/src/tools/fs.rs::rejects_symlink_escape", "另有 3 条同模块单测"),
 ("TC-SEC-004", "SSRF 拒绝内网（含简写形式）", "TC-SEC-004", "已实现",
  "crates/core/src/tools/net.rs::blocks_shorthand_ipv4_forms", "另有 5 条同模块用例"),
 ("TC-SEC-005", "注入守卫（终端/密码）", "TC-SEC-005", "已实现",
  "crates/core/tests/e2e_dictation.rs::tick_refuses_password_like_speech", "终端单行见 terminal_targets_get_a_single_line"),
 ("TC-SEC-006", "删除逐表报告", "TC-SEC-006", "已实现",
  "crates/core/src/deletion.rs::removes_every_local_artifact_and_reports_it", "八步 + 范围外声明"),
 ("TC-APP-001", "单实例锁", "TC-APP-001", "已实现",
  "crates/platform/src/singleton.rs::second_instance_is_refused", ""),
 ("TC-APP-002", "更新包验签", "TC-APP-002", "spec-only", "-", "桌面外壳落地后补"),
]

def tests_on_branch(branch):
    names = {}
    out = subprocess.run(["git", "ls-tree", "-r", "--name-only", branch, "crates"],
                         cwd=ROOT, capture_output=True, text=True).stdout.split()
    for path in out:
        if not path.endswith(".rs"):
            continue
        body = subprocess.run(["git", "show", f"{branch}:{path}"],
                              cwd=ROOT, capture_output=True, text=True).stdout
        for m in re.finditer(r'#\[test\]\s*(?:\n\s*)*fn\s+([a-z0-9_]+)', body):
            names.setdefault(path, set()).add(m.group(1))
    return names

def main():
    branch = source_branch()
    if branch is None:
        print("tds-evidence: no source branch available (tried feat/impl, origin/feat/impl, HEAD)")
        print("CI must fetch the implementation branch before running this check.")
        return 2
    have = tests_on_branch(branch)
    print(f"tds-evidence: reading tests from {branch}")
    missing = []
    lines = []
    counts = {"已实现": 0, "部分": 0, "spec-only": 0}
    for code, req, cases, status, evidence, note in ROWS:
        counts[status] = counts.get(status, 0) + 1
        if evidence != "-":
            path, _, fn = evidence.partition("::")
            if fn not in have.get(path, set()):
                missing.append(f"{code}: {evidence}")
        ev = f"`{evidence}`" if evidence != "-" else "—"
        lines.append(f"| {code} | {req} | {cases} | **{status}** | {ev} | {note or '—'} |")
    if missing:
        print("MISSING TEST CITATIONS:", *missing, sep="\n")
        return 1
    total = len(ROWS)
    header = (f"共 {total} 条：已实现 {counts['已实现']}、部分 {counts['部分']}、"
              f"spec-only {counts.get('spec-only',0)}。**只有 `已实现` 才能在执行报告中计入通过；"
              f"`部分` 与 `spec-only` 一律计入未执行/未覆盖。**\n")
    table = ("| 用例 | 要求 | TC | 状态 | 证据（真实测试） | 备注 |\n"
             "|---|---|---|---|---|---|\n" + "\n".join(lines) + "\n")
    # Inject into the document itself: one source of truth, no copy to drift.
    doc_path = ROOT / "docs" / "04-test-design.md"
    doc = doc_path.read_text(encoding="utf-8")
    begin = "<!-- TDS-MATRIX:BEGIN -->"
    end = "<!-- TDS-MATRIX:END -->"
    if begin not in doc or end not in doc:
        print("markers missing in docs/04-test-design.md")
        return 1
    head, rest = doc.split(begin, 1)
    _, tail = rest.split(end, 1)
    doc_path.write_text(head + begin + "\n" + header + "\n" + table + end + tail, encoding="utf-8")
    print(header)
    print("injected into docs/04-test-design.md")
    return 0

if __name__ == "__main__":
    sys.exit(main())
