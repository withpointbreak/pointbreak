# Substrate Thesis Summary

## Status

Source-facing summary. Shore's substrate thesis is supported by the current code-review system and
one headless agent task-supervision prototype. Treat the framing as stable internal architecture
language, not as a universal product claim.

The full architecture still evolves through small implementation plans, code review, and tests.
This summary explains why the current code uses substrate-shaped event, projection, identity,
freshness, and advisory/operative patterns.

## Thesis

Shore is a durable shared medium for software work objects. The same substrate pattern can support
more than one software-work domain without growing a central workflow controller:

- append-only event log;
- stable work-object identity;
- attributed assertions with provenance;
- actor- or purpose-specific projections;
- explicit interpretive, attention, and executive policy;
- stale-state detection through captured identity and fingerprints.

The current evidence supports this thesis for code review and a first task-supervision prototype.
Future domains should be treated as new stress tests, not as automatic extensions.

## What The Prototype Proved

The task-supervision prototype added a second domain without replacing the review-domain model:

- `ReviewUnit` and `TaskAttempt` share `WorkObjectId` / `WorkObjectType` identity.
- `TargetRef` carries domain-specific target shapes without forcing one serialization layout.
- Task-domain events use the same `ShoreEvent` envelope as review-domain events.
- The adapter maps Claude Code session JSON into deterministic task intents and then into
  `ShoreEvent`s.
- Task projections are sibling read-side views, not extensions of `SessionState` or review
  history.
- The resumption decision lives in one named projection, with explicit diagnostics and fail-closed
  behavior.
- No scheduler, hard lease, write gate, or global `current_task_attempt_id` was needed.

The important result is not that task supervision is now a product surface. The result is that the
event-log and projection substrate carried a second domain cleanly enough to promote the vocabulary
to source-facing internal docs.

## Success Criteria

The prototype supports the thesis within its scope:

| Criterion | Result |
|---|---|
| Same event-log / projection pattern works for review units and task attempts. | Supported. |
| Humans and agents coordinate asynchronously through recorded facts, not direct calls. | Supported. |
| Stale resolutions can be detected with work-object identity and fingerprints. | Supported at the substrate-mechanism level. |
| Task state can be understood from projections without raw transcripts. | Supported within the tested fixture set. |
| Assertions stay advisory by default; operative status is policy-derived. | Supported. |
| No scheduler, hard leases, or controller-like state are required. | Supported. |
| Real agent output maps with acceptable loss. | Supported within prototype scope. |

Two qualifications matter:

- The prototype exercised a single second domain. That supports the internal language; it does not
  prove every future domain will fit.
- Claude-session adapter events do not yet populate real code-state fingerprints. The
  fingerprint-based stale-resolution mechanism is proven by tests that populate fingerprints, while
  real imported Claude logs currently fall back to checkpoint-identity freshness when fingerprints
  are absent.

## Load-Bearing Decisions

### No Hidden Controller

Shore should not silently become a workflow engine. Executive policy belongs in named projections or
explicit ADRs. A projection may answer whether an actor can proceed, but the rule must be visible,
testable, and diagnostic-rich.

### No Global Current Task

Some review-domain projections have natural current-state values. Task supervision does not assume
that shape. Multiple attempts, checkpoints, or resolutions may be valid facts at the same time; the
projection should preserve ambiguity instead of introducing a scalar `current_task_attempt_id`.

### Advisory First

Recorded assertions are advisory by default. A projection can treat a fact as operative only under
explicit policy, such as a user-authored operative approval targeted at a fresh task checkpoint.

See [ADR-0003](adr/adr-0003-agent-resource-claims-advisory-first.md) for the corresponding
resource-claim decision.

### Fingerprints Are Opaque

Freshness checks compare fingerprints with equality. They should not parse domain meaning out of
the fingerprint string. If either side lacks a fingerprint, projections fall back to the relevant
identity rule.

### Domain Terms Stay At The Surface

The substrate gives contributors shared architecture language. It should not leak into user-facing
commands unless the term is genuinely clearer than the domain term.

## What This Does Not Authorize

- Productizing task supervision.
- Adding a `shore task` command family.
- Factoring a substrate crate or SDK.
- Adding hard leases, schedulers, write gates, or daemon-owned workflow state.
- Renaming review-domain code and commands outside a focused follow-up plan.

## Related Docs

- [Substrate Language](substrate-language.md)
- [ADR-0003: Agent Resource Claims Are Advisory by Default](adr/adr-0003-agent-resource-claims-advisory-first.md)
