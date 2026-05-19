# ADR-0003: Agent Resource Claims Are Advisory by Default

**Status:** Accepted
**Date:** 2026-05-19

## Context

Shore coordinates software work through durable facts and derived projections, not through a
central workflow controller. As agent workflows become more common, agents may need to communicate
intent such as "I am editing this file" or "I am working on this task checkpoint."

Hard leases and reservations would prevent some conflicts, but they would also introduce executive
policy with write-side force: a scheduler, lock manager, daemon broker, or first-claimer-wins rule.
That is not Shore's V1 architecture. Shore's substrate should preserve facts, surface conflicts,
and let readers apply explicit policy.

The existing writer contract already follows this posture. `.shore/` assumes one active Shore
writer per store at a time, but it does not coordinate broader multi-agent work through lockfiles,
leases, daemon brokering, IPC, or filesystem notifications.

## Decision

Agent resource claims are advisory by default.

An agent may record an intent to edit, hold, inspect, or otherwise act on a target. That fact is an
attributed assertion in the event log. It is not a lease grant, reservation token, scheduler command,
or write-side gate.

Concretely:

- Resource-claim assertions use advisory mode unless a specific projection policy treats them as
  operative.
- Projections surface conflicting claims as explicit conflict or ambiguity state.
- Readers decide how to behave from the projection they read.
- Recovery from stale or conflicting claims happens through later events: supersession, retraction,
  intervention resolution, human review, or a projection-specific authority rule.
- Shore does not block event writes because a competing advisory claim exists.

## Consequences

### Accepted

- Some conflicts can happen. Shore records and surfaces them; it does not prevent all of them.
- Agents need reader-side discipline. An agent that ignores advisory conflict projections may still
  produce collisions.
- Human review and later corrective facts remain the recovery path for conflicts that cannot be
  resolved mechanically.
- A future projection can summarize active resource claims, stale claims, or conflicting claims
  without changing the storage authority.

### Rejected

- `LeaseGranted` / `LeaseExpired` event types for V1.
- A first-claim-wins idempotency rule for competing resource claims.
- A write-side gate that rejects events while a conflicting claim is open.
- A central scheduler, daemon-owned workflow state, or lock manager as part of the substrate.

## Revisit Triggers

Reopen this decision if one of these occurs:

- Advisory-only coordination produces unrecoverable state, not merely inconvenient cleanup.
- Supersession or human review is structurally inadequate for a real recurring conflict.
- Resource-claim volume becomes a noise floor that makes projections unreadable.
- A concrete multi-agent workflow needs scope-bounded authority that cannot be expressed through
  actor, target, assertion mode, source provenance, and projection policy.
- A remote or multi-process storage backend introduces measured write-conflict behavior that cannot
  be handled by event idempotency and replay.

If this decision is reopened, the next design should still name the executive-policy exception
directly. It should not silently introduce lock behavior as ordinary metadata.

## Related Docs

- [Substrate Language](../substrate-language.md)
- [Substrate Thesis Summary](../substrate-thesis-summary.md)
