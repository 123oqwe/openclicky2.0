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
    CASES_RUN=$((CASES_RUN + 1))

    local clean_out clean_rc
    clean_out="$(eval "$check_cmd" 2>&1)"
    clean_rc=$?
    if [ "$clean_rc" -ne 0 ]; then
        echo "FAIL $name: the check already fails on a clean tree"
        printf '%s\n' "$clean_out" | tail -5
        return 1
    fi

    local broke_out broke_rc
    broke_out="$(MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' eval "$break_cmd" 2>&1)"
    broke_rc=$?
    if [ "$broke_rc" -ne 0 ]; then
        # A break that failed to apply must never be mistaken for a check that
        # cannot fail. Every break command asserts its target before touching it.
        echo "FAIL $name: the break command itself failed"
        printf '%s\n' "$broke_out" | tail -5
        git checkout -- . >/dev/null 2>&1
        return 1
    fi

    local broken_out broken_rc
    broken_out="$(eval "$check_cmd" 2>&1)"
    broken_rc=$?
    # Before the restore: after it, the diff is always empty, which is how "the break
    # changed: nothing" got printed for a break that had in fact worked.
    local break_effect
    break_effect="$(git diff --stat)"
    git checkout -- . >/dev/null 2>&1

    # Two questions, two checks. A tracked file that came back different is one
    # thing; a new untracked file left behind is another. Comparing the whole
    # porcelain string conflated them, and a CRLF checkout reports files as modified
    # that `git diff` considers unchanged.
    if ! git diff --quiet || ! git diff --cached --quiet; then
        echo "FAIL $name: a tracked file was not restored"
        git diff --stat | head -5
        return 1
    fi
    if [ "$(git ls-files --others --exclude-standard | sort)" != "${UNTRACKED_BASELINE-}" ]; then
        echo "FAIL $name: the tree was not restored; the next case would run on a dirty tree"
        diff <(printf '%s\n' "${UNTRACKED_BASELINE-}") \
             <(git ls-files --others --exclude-standard | sort) | head -6
        return 1
    fi

    if [ "$broken_rc" -ne 0 ]; then
        case "$check_cmd" in
        cargo*)
            if ! printf '%s\n' "$broken_out" | grep -qE 'test result: FAILED|--- FAILED'; then
                echo "FAIL $name: the mutation broke the build instead of the test; that proves nothing"
                printf '%s\n' "$broken_out" | tail -5
                return 1
            fi
            ;;
        esac
    fi

    if [ "$broken_rc" -eq 0 ]; then
        echo "FAIL $name: still green after the break, so the check does not test this"
        echo "  the break changed:"
        if [ -n "$break_effect" ]; then
            printf '%s\n' "$break_effect" | sed 's/^/    /'
        else
            echo "    (nothing)"
        fi
        echo "  the check printed:"
        printf '%s\n' "$broken_out" | tail -8 | sed 's/^/    /'
        return 1
    fi

    report "$name: clean" "exit 0"
    local summary
    summary="$(printf '%s\n' "$broken_out" | grep -vE '^[[:space:]]*$' | grep -m1 -E 'VIOLATION|error|FAILED|Traceback' || true)"
    [ -n "$summary" ] || summary="(no diagnostic line; exit $broken_rc)"
    report "$name: broken" "exit $broken_rc — $summary"
    return 0
}

case "${1:-}" in
--self-check)
    refuse_if_dirty
    UNTRACKED_BASELINE="$(git ls-files --others --exclude-standard | sort)"
    CASES_RUN=0
    # The failure mode this whole script exists to catch is "the break did nothing, the
    # check stayed green, and the pair was counted as proof". So: run a case whose break
    # is `true` and require the harness to say so. If it reports success here, the
    # harness is the thing that is broken.
    out="$(prove "self-check" "true" "true" 2>&1)"
    rc=$?
    printf '%s\n' "$out"
    printf '  self-check: prove exited %s (non-zero means it noticed)\n' "$rc"
    case "$out" in
        *"still green after the break"*)
            if [ "$rc" -eq 0 ]; then
                echo "FAIL self-check: the harness accepted a break that did nothing"
                exit 1
            fi
            echo "self-check: OK — a break that does nothing is reported, not counted"
            exit 0
            ;;
        *)
            echo "FAIL self-check: expected the harness to report a still-green break"
            exit 1
            ;;
    esac
    ;;
--all)
    refuse_if_dirty
    UNTRACKED_BASELINE="$(git ls-files --others --exclude-standard | sort)"
    status=0
    CASES_RUN=0
    # Each break asserts its own target (scripts/break-one.py), so a break that cannot
    # apply fails as a break instead of reading as a check that never fails.
    case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
        echo "  skipped: the | sh line is unix only, and a CRLF checkout cannot run it"
        ;;
    *)
        prove "install-sh-guard" \
            'python3 scripts/break-one.py scripts/install.sh "set -eu" "set -eu\nBASHISM=(a b c)"' \
            "PATH=\"$ROOT/scripts/test-shims:$PATH\" bash scripts/check-install-under-sh.sh" || status=1
        ;;
    esac
    prove "version-agreement" \
        "true" \
        "bash scripts/check-release-versions.sh" || status=1
    prove "platform-gates" \
        'python3 scripts/break-one.py crates/core/tests/e2e_agent.rs "#[cfg(unix)]" ""' \
        "python3 scripts/check-platform-gates.py" || status=1
    prove "inv-d1-capture-gate" \
        'python3 scripts/break-one.py crates/core/src/privacy.rs "self.opens.fetch_add(1, Ordering::SeqCst);" "let _ = Ordering::SeqCst;"' \
        "cargo test -q -p openclicky-core capture_gate_counts_only_user_actions" || status=1
    prove "one-ask-one-unit" \
        'python3 scripts/break-one.py crates/core/src/usage.rs "let waived = self.is_waived(input);" "let waived = true;"' \
        "cargo test -q -p openclicky-core voice_costs_one_unit" || status=1
    prove "outside-write-agrees" \
        'python3 scripts/break-one.py crates/core/src/tools/fs.rs "-> bool {" "-> bool { return false;"' \
        "cargo test -q -p openclicky-core the_policy_and_the_writer_agree_about_what_is_outside" || status=1

    if [ "${CASES_RUN:-0}" -lt 5 ]; then
        echo "prove-can-fail: FAILED — only $CASES_RUN cases ran; this harness must run at least 5"
        exit 1
    fi
    if [ "$status" -ne 0 ]; then
        echo "prove-can-fail: FAILED"
        exit 1
    fi
    echo "prove-can-fail: ${CASES_RUN} guards, every one went red when its fix was removed"
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
