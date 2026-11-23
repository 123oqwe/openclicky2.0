# AGENTS.md — OpenClicky 2.0

Read this before any change. It is the operational summary of `docs/03-agent-playbook.md`.

## Order of authority

1. Source product PRD (see `docs/00-baseline.md`) — product behaviour
2. `docs/01-tech-prd.md` — technical commitments
3. `docs/adr/` (latest Accepted) — decisions and their costs
4. `docs/02-sdd.md` — module internals, fields, algorithms
5. `docs/04-test-design.md` — test expectations
6. Your own summary — **never** evidence

## Non-negotiables

- No screen capture unless triggered by a user hotkey. `CaptureGate` is the only entry point (`crates/core/src/privacy.rs`).
- `crates/core` must not depend on Tauri, GUI, `ureq`, or `std::process`. Enforced by `scripts/check-invariants.sh`.
- Platform-specific code only in `crates/platform`.
- API keys never in code, logs, config, tests, or commits. Logs go through `security::redact`.
- No secrets in test fixtures. No network in tests. No `sleep` in tests — inject `Clock`.
- Tests before implementation. Red → green → refactor.
- Never delete or weaken a test to get green. If expectations changed, update `docs/04-test-design.md` first and say why.

## Before you code

1. Find the task in the current work item / issue; confirm scope and non-goals.
2. Read the SDD sections for the modules you touch, plus any related ADR.
3. Write the failing test(s) first.
4. Check you are not editing a shared-write area without coordination: `ports.rs`, `store/migrations.rs`, `docs/01-tech-prd.md`, `docs/02-sdd.md`, `Cargo.toml`, `pnpm-lock.yaml`, `.github/workflows/`.

## Before you claim done

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
bash scripts/check-invariants.sh
pnpm -C apps/desktop/ui typecheck && pnpm -C apps/desktop/ui test
```

Then fill the PR template: what changed, what did **not** change, dependencies, test output, untested items, risk, rollback. "It works" is not evidence.

## Commit style

Conventional Commits: `feat|fix|docs|test|chore|refactor(scope): subject`.
