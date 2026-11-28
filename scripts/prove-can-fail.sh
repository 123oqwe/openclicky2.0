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

# One list, two readers. `--all` runs it and `--list` prints it, so a case that is
# dropped from the run while still being listed cannot pass unnoticed. A hand-written
# second list is how protected-paths went missing while the floor still passed.
CASES=(
"install-sh-guard|python3 scripts/break-one.py scripts/install.sh \"set -eu\" \"set -eu\\nBASHISM=(a b c)\"|bash scripts/check-install-under-sh.sh"
"version-agreement|python3 scripts/break-one.py apps/desktop/tauri.conf.json '\"version\": \"2.1.1\",' '\"version\": \"9.9.9\",'|bash scripts/check-release-versions.sh"
"protected-paths|python3 scripts/break-one.py crates/core/src/tools/fs.rs \"refuse_if_protected(&path, protected)?;\" \"let _ = (&path, &protected);\"|cargo test -q -p openclicky-core an_approved_outside_write_still_cannot_touch"
"platform-gates|python3 scripts/break-one.py crates/core/tests/e2e_agent.rs \"#[cfg(unix)]\" \"\"|python3 scripts/check-platform-gates.py"
"inv-d1-capture-gate|python3 scripts/break-one.py crates/core/src/privacy.rs \"self.opens.fetch_add(1, Ordering::SeqCst);\" \"let _ = Ordering::SeqCst;\"|cargo test -q -p openclicky-core capture_gate_counts_only_user_actions"
"one-ask-one-unit|python3 scripts/break-one.py crates/core/src/usage.rs \"let waived = self.is_waived(input);\" \"let waived = true;\"|cargo test -q -p openclicky-core voice_costs_one_unit"
"outside-write-agrees|python3 scripts/break-one.py crates/core/src/tools/fs.rs \"-> bool {\" \"-> bool { return false;\"|cargo test -q -p openclicky-core the_policy_and_the_writer_agree_about_what_is_outside"
)

# The install line cannot run from a CRLF checkout.
is_windows() {
    case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) return 0 ;;
    *) return 1 ;;
    esac
}

# install-sh-guard is a real dash run. Windows has no such shell. A machine
# without dash — GitHub's macOS image does not ship one — used to take the
# check's skip branch and exit 0, so the break stayed green. A skipped case is
# not a proof; leave it out of the run.
install_guard_runs() {
    is_windows && return 1
    command -v dash >/dev/null 2>&1 || return 1
    return 0
}

usage() {
    echo "usage: scripts/prove-can-fail.sh --list | --self-check | --all" >&2
    echo "       scripts/prove-can-fail.sh <name> -- <break command> -- <check command>" >&2
    exit 2
}

case "${1:-}" in
--list)
    for entry in "${CASES[@]}"; do
        case "$entry" in
        install-sh-guard*) install_guard_runs || continue ;;
        esac
        printf '%s\n' "${entry%%|*}"
    done
    exit 0
    ;;
--self-check)
    refuse_if_dirty
    CASES_RUN=0
    UNTRACKED_BASELINE="$(git ls-files --others --exclude-standard | sort)"
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
    listed=0
    for entry in "${CASES[@]}"; do
        case "$entry" in
        install-sh-guard*) install_guard_runs || continue ;;
        esac
        listed=$((listed + 1))
        name="${entry%%|*}"
        rest="${entry#*|}"
        break_cmd="${rest%%|*}"
        check_cmd="${rest#*|}"
        case "$name" in
        install-sh-guard*)
            break_cmd="PATH=\"$ROOT/scripts/test-shims:$PATH\" $break_cmd"
            check_cmd="PATH=\"$ROOT/scripts/test-shims:$PATH\" $check_cmd"
            ;;
        esac
        # PROVE_DEMO_* exist so a demonstration never edits a file. Three times in this
        # session a demo mutation was still in the tree when the commit was made.
        if [ "${PROVE_DEMO_NOOP:-0}" = "1" ] && [ "$CASES_RUN" -eq 0 ]; then
            break_cmd="true"
        fi
        if [ -n "${PROVE_DEMO_TRUNCATE:-}" ] && [ "$CASES_RUN" -ge "${PROVE_DEMO_TRUNCATE}" ]; then
            break
        fi
        prove "$name" "$break_cmd" "$check_cmd" || status=1
    done
    if [ "${CASES_RUN:-0}" -lt 5 ]; then
        echo "prove-can-fail: FAILED — only $CASES_RUN cases ran; this harness must run at least 5"
        exit 1
    fi
    if [ "$CASES_RUN" -ne "$listed" ]; then
        echo "prove-can-fail: FAILED — $listed cases are listed but $CASES_RUN ran; a case was dropped or skipped silently"
        exit 1
    fi
    if [ "$status" -ne 0 ]; then
        echo "prove-can-fail: FAILED"
        exit 1
    fi
    echo "prove-can-fail: ${CASES_RUN} guards, every one went red when its fix was removed"
    exit 0
    ;;
*)
    # Anything else used to fall off the end of this case and exit 0, which is
    # a green run that proved nothing. The single-case form in the header is
    # the one shape that does run a proof; a typo or a missing argument does not.
    name="${1:-}"
    if [ -z "$name" ] || [ "$name" = "--" ]; then
        echo "prove-can-fail: missing arguments" >&2
        usage
    fi
    shift
    if [ "${1:-}" != "--" ]; then
        echo "prove-can-fail: unknown arguments: $name${*:+ $*}" >&2
        usage
    fi
    shift
    break_parts=()
    while [ $# -gt 0 ] && [ "$1" != "--" ]; do
        break_parts+=("$1")
        shift
    done
    if [ "${1:-}" != "--" ] || [ ${#break_parts[@]} -eq 0 ] || [ $# -lt 2 ]; then
        echo "prove-can-fail: expected <name> -- <break command> -- <check command>" >&2
        usage
    fi
    shift
    printf -v break_cmd '%q ' "${break_parts[@]}"
    printf -v check_cmd '%q ' "$@"
    refuse_if_dirty
    CASES_RUN=0
    UNTRACKED_BASELINE="$(git ls-files --others --exclude-standard | sort)"
    prove "$name" "$break_cmd" "$check_cmd"
    exit $?
    ;;
esac
