#!/usr/bin/env bash
# A guard nobody has watched fail is a guard nobody knows works.
#
#   scripts/prove-can-fail.sh <name> -- <break command> -- <check command>
#   scripts/prove-can-fail.sh --all
#
# Runs the check on a clean tree (it must pass), applies the break, runs the check
# again (it must fail), then restores. Both halves are printed, and either one going
# the wrong way is a failure: a check that never fails proves nothing, and a break that
# does not break means the wiring is not what its author thought.
#
# The tree has to be clean before this starts. Breaking things on top of someone's
# work in progress is how a proof turns into an incident.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

refuse_if_dirty() {
    if ! git diff --quiet || ! git diff --cached --quiet; then
        echo "prove-can-fail: the tree has uncommitted changes; commit or stash first" >&2
        exit 2
    fi
}

# Two halves, both printed. `label` is what the log line says, so a failure names
# which half went the wrong way rather than just "red".
report() {
    printf '  %-28s %s\n' "$1" "$2"
}

prove() {
    local name="$1" break_cmd="$2" check_cmd="$3"

    local clean_out clean_rc
    clean_out="$(eval "$check_cmd" 2>&1)"
    clean_rc=$?
    if [ "$clean_rc" -ne 0 ]; then
        echo "FAIL $name: the check already fails on a clean tree"
        printf '%s\n' "$clean_out" | tail -5
        return 1
    fi

    local broke_out broke_rc
    broke_out="$(eval "$break_cmd" 2>&1)"
    if [ $? -ne 0 ]; then
        echo "FAIL $name: the break command itself failed"
        printf '%s\n' "$broke_out" | tail -5
        git checkout -- . >/dev/null 2>&1
        return 1
    fi

    local broken_out broken_rc
    broken_out="$(eval "$check_cmd" 2>&1)"
    broken_rc=$?
    git checkout -- . >/dev/null 2>&1

    if [ "$broken_rc" -eq 0 ]; then
        echo "FAIL $name: still green after the break, so the check does not test this"
        return 1
    fi

    report "$name: clean" "exit 0"
    report "$name: broken" "exit $broken_rc — $(printf '%s\n' "$broken_out" | grep -m1 -E 'VIOLATION|error|FAILED' || true)"
    return 0
}

case "${1:-}" in
--all)
    refuse_if_dirty
    status=0
    # Each line: name, what to break, what to run. The break is always something a
    # person could plausibly commit by accident, not a sabotage of the test itself.
    prove "install-sh-guard" \
        "sed -i.bak 's/^set -eu\$/set -euo pipefail/' scripts/install.sh && rm -f scripts/install.sh.bak" \
        "FORCE_SH_CHECK=1 bash scripts/check-invariants.sh" || status=1
    prove "version-agreement" \
        "python3 -c \"import json,pathlib;p=pathlib.Path('apps/desktop/tauri.conf.json');d=json.loads(p.read_text());d['version']='9.9.9';p.write_text(json.dumps(d,indent=2)+chr(10))\"" \
        "bash scripts/check-invariants.sh" || status=1
    prove "protected-paths" \
        "sed -i.bak 's/^    refuse_if_protected(&path, protected)?;\$/    \\/\\/ removed/' crates/core/src/tools/fs.rs && rm -f crates/core/src/tools/fs.rs.bak" \
        "cargo test -p openclicky-core --test e2e_agent an_approved_outside --quiet" || status=1
    prove "platform-gates" \
        "sed -i.bak 's/^    #\\[cfg(unix)\\]\$/    \\/\\/ gate removed/' crates/core/tests/e2e_agent.rs && rm -f crates/core/tests/e2e_agent.rs.bak" \
        "python3 scripts/check-platform-gates.py" || status=1
    if [ "$status" -ne 0 ]; then
        echo "prove-can-fail: FAILED"
        exit 1
    fi
    echo "prove-can-fail: OK — every guard above went red when its fix was removed"
    exit 0
    ;;
esac

name="${1:-}"
[ -n "$name" ] || {
    echo "usage: prove-can-fail.sh <name> -- <break command> -- <check command>" >&2
    echo "       prove-can-fail.sh --all" >&2
    exit 2
}
shift
[ "${1:-}" = "--" ] && shift
break_cmd=""
while [ $# -gt 0 ] && [ "$1" != "--" ]; do
    break_cmd="$break_cmd $1"
    shift
done
[ "${1:-}" = "--" ] && shift
check_cmd="$*"
[ -n "$break_cmd" ] && [ -n "$check_cmd" ] || {
    echo "prove-can-fail: needs both a break and a check, separated by --" >&2
    exit 2
}
refuse_if_dirty
prove "$name" "$break_cmd" "$check_cmd" || exit 1
echo "prove-can-fail: $name OK"
