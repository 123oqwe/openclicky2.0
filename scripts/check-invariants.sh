#!/usr/bin/env bash
# Structural invariants that must hold regardless of who wrote the code.
# Runs on every PR and locally before "done". Exit non-zero on any violation.
set -uo pipefail

fail=0
note() { printf '%s\n' "$*"; }
bad()  { printf 'VIOLATION: %s\n' "$*"; fail=1; }

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

has_code=0
[ -f Cargo.toml ] && has_code=1

# ---------------------------------------------------------------- history
# A squash merge copies the whole commit body into main. A "Closes #63" trailer
# therefore closes the issue without anyone deciding to, which happened on
# 498e39a: the pull request said Refs and the commit body closed it anyway.
# Closing intent belongs in the pull request, where it can be reviewed.
note "== history: no auto-close keyword in a commit body =="
main_ref=$(git rev-parse --verify --quiet origin/main || true)
if [ -z "$main_ref" ]; then
  note "   skipped: this clone has no origin/main to compare against"
else
  closers=$(git log --format=%B "$main_ref"..HEAD 2>/dev/null \
            | grep -inE '^[[:space:]]*(closes|fixes|resolves)[[:space:]]+#' || true)
  [ -z "$closers" ] || bad "commit body carries an auto-close keyword and a squash merge will act on it: $closers"
fi

# ---------------------------------------------------------------- docs
# Every file path referenced by an ADR or by the baseline must exist, so that
# "see 06-acceptance.md" never points at nothing.
note "== docs: referenced deliverables exist =="
for f in docs/00-baseline.md docs/01-tech-prd.md docs/02-sdd.md docs/03-agent-playbook.md docs/04-test-design.md; do
  [ -f "$f" ] || bad "missing required document $f"
done
for p in docs/05-deployment.md docs/06-acceptance.md; do
  [ -f "$p" ] || bad "missing referenced deliverable $p"
done

note "== docs: numbering and coverage =="
python3 scripts/doc-audit.py >/dev/null || bad "doc-audit failed"
python3 scripts/criteria-coverage.py >/dev/null || bad "criteria-coverage failed"
python3 scripts/ref-check.py >/dev/null || bad "broken cross-reference (section, ADR id or path)"

note "== tds: matrix must match the real tests =="
if ! python3 scripts/tds-evidence.py >/dev/null; then
  bad "tds-evidence failed (a cited test does not exist)"
fi
if ! git diff --quiet -- docs/04-test-design.md; then
  bad "docs/04-test-design.md matrix is stale; run scripts/tds-evidence.py and commit"
fi

# ---------------------------------------------------------------- secrets
note "== secrets: no key-shaped literals in the repo =="
# A real key is >= 20 chars of printable non-space after a known prefix.
if command -v rg >/dev/null 2>&1; then
  hits=$(rg -n --hidden -g '!*.json' -g '!docs/**' -g '!.git/**' -g '!target/**' -g '!node_modules/**' \
        '(sk-[A-Za-z0-9]{20,}|xox[bap]-[A-Za-z0-9-]{10,}|ghp_[A-Za-z0-9]{20,})' . || true)
else
  hits=$(grep -rnE '(sk-[A-Za-z0-9]{20,}|xox[bap]-[A-Za-z0-9-]{10,}|ghp_[A-Za-z0-9]{20,})' \
        --exclude-dir=.git --exclude-dir=docs --exclude-dir=target --exclude-dir=node_modules \
        --exclude='*.json' . || true)
fi
[ -z "$hits" ] || bad "possible credential literal: $hits"

# ---------------------------------------------------------------- code
if [ "$has_code" -eq 1 ]; then
  note "== core: no GUI / HTTP / process dependencies =="
  core_manifest="crates/core/Cargo.toml"
  if [ -f "$core_manifest" ]; then
    for banned in tauri reqwest ureq tokio hyper; do
      if grep -qE "^${banned}[[:space:]]*=" "$core_manifest"; then
        bad "crates/core depends on $banned (must stay pure)"
      fi
    done
  fi

  note "== core: no direct std::process usage (comments ignored) =="
  proc_hits=$(grep -rn --include='*.rs' 'std::process' crates/core/src 2>/dev/null \
              | grep -vE ':[0-9]+: *//' || true)
  [ -z "$proc_hits" ] || bad "crates/core uses std::process: $proc_hits"

  note "== core: every module is wired, or declares UNWIRED =="
  python3 scripts/check-wiring.py --quiet \
    || bad "an unwired core module has no UNWIRED marker (scripts/check-wiring.py)"

  note "== capture: every call site passes a guard; only voice or an explicit user action =="
  # Ignore comments; a real call looks like `x.capture_all(&guard)`.
  calls=$(grep -rn --include='*.rs' 'capture_all(' crates/core/src crates/platform/src apps 2>/dev/null \
          | grep -vE ':[0-9]+: *//' | grep -v 'fn capture_all' || true)
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    case "$line" in
      *'crates/core/src/voice.rs'*) ;;                                   # the orchestrator
      *'apps/cli/src/main.rs'*|*'apps/desktop/src/main.rs'*) ;;          # selftest: user-initiated Calibration
      *) echo "VIOLATION: capture outside the allowed call sites: $line"; fail=1 ;;
    esac
  done <<< "$calls"
  if echo "$calls" | grep -v 'capture_all(&' >/dev/null 2>&1; then
    bad "capture_all called without a &guard argument"
  fi

  note "== overlay: a pointer is drawn where the plan says it is =="
  # The overlay window is not at the screen's origin: macOS clamps it below the
  # menu bar, and a second display puts it somewhere else again. A canvas that
  # assumes its own top-left is (0,0) draws every stroke that far off, which is
  # #140. The transform that carries the origin has a case in a node test.
  if command -v node >/dev/null 2>&1; then
    if ! overlay_out=$(node --test apps/desktop/ui/overlay-coords.test.js 2>&1); then
      echo "$overlay_out" | tail -20
      bad "the overlay transform does not put a plan point back on its screen point"
    fi
  else
    note "   skipped: no node on this machine"
  fi

  note "== linux deps come from one script =="
  # Installing the built artifact itself is fine; declaring a second package
  # list is what drifted. A path argument marks the former.
  inline_deps=$(grep -n 'apt-get install' .github/workflows/*.yml 2>/dev/null \
                | grep -vE '\./|/dist/' || true)
  [ -z "$inline_deps" ] || bad "a workflow installs packages inline; call scripts/linux-deps.sh ($inline_deps)"

  note "== platform code sits behind a platform gate =="
  # Twice now a change compiled on macOS and Linux and failed on Windows: a set of
  # types in a struct literal, then std::os::unix in a test body. `clippy --all-targets`
  # on macOS cannot see either, because the code never compiles there.
  python3 scripts/check-platform-gates.py >/dev/null \
    || bad "a platform-only path has no cfg gate (scripts/check-platform-gates.py)"

  note "== new public surface has a line in the test matrix =="
  python3 scripts/check-new-tests.py \
    || bad "a new public function has no case in docs/04-test-design.md"

  note "== invariant cases can fail =="
  if [ "${RUN_INVARIANT_MUTATIONS:-0}" = "1" ]; then
    mutation_out=$(bash scripts/prove-can-fail.sh --all 2>&1) || {
      echo "$mutation_out"
      bad "an invariant case cannot fail, so it proves nothing"
    }
    self_out=$(bash scripts/prove-can-fail.sh --self-check 2>&1) || {
      echo "$self_out"
      bad "the harness accepts a break that does nothing, so it proves nothing"
    }
    echo "$mutation_out"
    # A switch that is on but proves nothing is the failure this whole rule
    # exists to prevent: an empty harness would otherwise report success.
    # Five or more, including 10-49: [5-9][0-9]* only matches a first digit of 5-9.
    if ! echo "$mutation_out" | grep -qE '^prove-can-fail: ([5-9]|[1-9][0-9]+) guards, every one went red'; then
      bad "the mutation harness reported no cases, so it checked nothing"
    fi
  else
    note "   skipped here (needs cargo); the rust jobs run it with RUN_INVARIANT_MUTATIONS=1"
  fi

  note "== the diff stays inside the declared task scope =="
  python3 scripts/check-scope.py \
    || bad "the diff touches files outside docs/.scope-allow"

  note "== permissions: probe before tool execution =="
  if ! grep -rn --include='*.rs' 'permission_probe\|PermissionProbe' crates/core/src >/dev/null 2>&1; then
    bad "no PermissionProbe port wired in core"
  fi

  note "== release: the crate and the bundle carry the same version =="
  bash scripts/check-release-versions.sh \
    || bad "Cargo.toml and tauri.conf.json disagree about the version"

  note "== macos: the bundle declares audio input, or TCC refuses to prompt =="
  # A probing code path is not enough. Without the entitlement, macOS answers
  # every microphone request by refusing, and no amount of UI can fix that.
  entitlements_ref=$(python3 -c 'import json;print(json.load(open("apps/desktop/tauri.conf.json")).get("bundle",{}).get("macOS",{}).get("entitlements",""))' 2>/dev/null || true)
  if [ -z "$entitlements_ref" ]; then
    bad "apps/desktop/tauri.conf.json declares no bundle.macOS.entitlements"
  else
    entitlements_path="apps/desktop/$entitlements_ref"
    if [ ! -f "$entitlements_path" ]; then
      bad "bundle.macOS.entitlements points at $entitlements_path, which does not exist"
    elif ! grep -q 'com.apple.security.device.audio-input' "$entitlements_path"; then
      bad "$entitlements_path does not grant com.apple.security.device.audio-input"
    fi
  fi

  note "== install: the command the README prints runs an install under sh =="
  bash scripts/check-install-under-sh.sh || bad "the README's | sh line does not install under dash"

  note "== overlay: no native fullscreen transition on a transparent window =="
  # `fullscreen: true` maps to Fullscreen::Borderless(None). tao runs
  # set_fullscreen before orderFront (window_delegate.rs:548-561): the transition
  # opens a new Space and takes the window with it. bounds and alpha stay correct,
  # onscreen goes false, and the canvas is never composited, so the symptom is
  # "the pointer does not draw" five layers away from the cause. #124.
  overlay_fs=$(python3 -c '
import json
w = json.load(open("apps/desktop/tauri.conf.json")).get("app",{}).get("windows",[])
print(next((str(x.get("fullscreen","")).lower() for x in w if x.get("label")=="overlay"), ""))
' 2>/dev/null || true)
  if [ "$overlay_fs" = "true" ]; then
    bad "the overlay window sets fullscreen: true, which parks it in another Space (#124)"
  fi
else
  note "== code invariants skipped: no Cargo.toml in this tree =="
fi

if [ "$fail" -ne 0 ]; then
  echo "invariants: FAILED"
  exit 1
fi
echo "invariants: OK"
