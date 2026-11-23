---
name: Task
about: One work item. All fields are required before work starts (playbook §04).
labels: task
---

## ID / name / work package
`T-___` · WP1 | WP2 | WP3 | WP4 | WP5 | WP6

## Goal / scope / non-goals

## References (pin versions)
- Product PRD:
- tech-prd §:
- ADR:
- SDD §:
- Test cases (TDS):

## Deliverables
- [ ] code
- [ ] tests
- [ ] docs

## Owner / reviewer
Owner: @
Reviewer (independent): @     <!-- not the same person/agent as owner -->

## Gates
- Development precondition:
- Merge precondition: required checks + dependencies merged
- Integration/release precondition (G2/G3):

## Allowed write scope
Paths this task may touch; anything else stops and asks.

## Parallelism and conflicts
Can run with: ____   Must not touch: ____ (shared-write areas: ports.rs, migrations.rs, tech-prd, SDD, Cargo.toml, workflows)

## Planned PR split
1.
2.

## Risk level and acceptance level
low / medium / high -> G1 / G1+G2

## Completion criteria and evidence
Commands to run; where the output lives.

## Estimate and resources
Time box, token budget, environment.

## Blockers / handoff
Current blocker, next step, owner.
