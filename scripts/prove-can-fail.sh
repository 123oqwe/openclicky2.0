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

# Every case lives in scripts/prove-can-fail.d/*.case, one line each:
#
#   platform: linux | darwin | windows | any     (optional, first line, default any)
#   name|break command|check command
#
# One directory, two readers. A case that exists but cannot be parsed is listed and
# not run, which is how protected-paths went missing while the floor still passed.
CASES_DIR="$ROOT/scripts/prove-can-fail.d"

platform_matches() {
    case "$1" in
    any | "") return 0 ;;
    linux) case "$(uname -s)" in Linux) return 0 ;; esac ;; 
    darwin) case "$(uname -s)" in Darwin) return 0 ;; esac ;;
    windows)
        case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) return 0 ;; esac
        ;;
    esac
    return 1
}

# The floor is a number in the directory, not in this script, so adding a case does
# not require editing code and deleting one is visible as a lower count.
floor_count() {
    if [ -f "$CASES_DIR/floor" ]; then
        tr -dc '0-9' <"$CASES_DIR/floor"
    else
        echo 0
    fi
}

case_files() {
    [ -d "$CASES_DIR" ] || return 0
    for f in "$CASES_DIR"/*.case; do
        [ -e "$f" ] || continue
        echo "$f"
    done
}

case_platform() {
    sed -n 's/^platform:[[:space:]]*//p' "$1" | head -1
}

case_line() {
    grep -v -e '^platform:' -e '^[[:space:]]*#' -e '^[[:space:]]*$' "$1" | head -1
}

case_name() {
    local line
    line="$(case_line "$1")"
    printf '%s' "${line%%|*}"
}

case_break() {
    local line rest
    line="$(case_line "$1")"
    rest="${line#*|}"
    printf '%s' "${rest%%|*}"
}

case_check() {
    local line
    line="$(case_line "$1")"
    printf '%s' "${line##*|}"
}

case "${1:-}" in
--list)
    for f in $(case_files); do
        platform_matches "$(case_platform "$f")" || continue
        case_name "$f"
        echo
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
    for f in $(case_files); do
        platform_matches "$(case_platform "$f")" || continue
        listed=$((listed + 1))
        name="$(case_name "$f")"
        if [ -z "$name" ]; then
            echo "FAIL $f: no name|break|check line"
            status=1
            continue
        fi
        break_cmd="$(case_break "$f")"
        check_cmd="$(case_check "$f")"
        # PROVE_DEMO_* so a demonstration never edits a file. Three times in this
        # session a demo mutation was still in the tree when the commit was made.
        if [ "${PROVE_DEMO_NOOP:-0}" = "1" ] && [ "$CASES_RUN" -eq 0 ]; then
            break_cmd="true"
        fi
        if [ -n "${PROVE_DEMO_TRUNCATE:-}" ] && [ "$CASES_RUN" -ge "${PROVE_DEMO_TRUNCATE}" ]; then
            break
        fi
        prove "$name" "$break_cmd" "$check_cmd" || status=1
        case "$name" in
        install-sh-guard) ;;
        esac
    done
    floor="$(floor_count)"
    if [ "$CASES_RUN" -ne "$listed" ]; then
        echo "prove-can-fail: FAILED — $listed cases are listed but $CASES_RUN ran; a case was dropped or skipped silently"
        exit 1
    fi
    # Counted from disk, not from what ran, because a platform-filtered case is a
    # legitimate absence and a deleted file is not.
    present="$(case_files | wc -l | tr -d ' ')"
    if [ "$present" -lt "$floor" ]; then
        echo "prove-can-fail: FAILED — $present .case files on disk but the floor is $floor; a case went missing"
        exit 1
    fi
    if [ "$status" -ne 0 ]; then
        echo "prove-can-fail: FAILED"
        exit 1
    fi
    echo "prove-can-fail: $CASES_RUN guards, every one went red when its fix was removed"
    exit 0
    ;;
esac
