# ADR-0004: Per-Event Ed25519 Signatures

**Status:** Accepted
**Date:** 2026-06-03
**See also:** [ADR-0003](./adr-0003-agent-resource-claims-advisory-first.md)

## Context

Shoreline events are durable review facts that can be forwarded between clones, bridges, and
library consumers. Existing event validation proves internal consistency: an event's `eventId`
matches its idempotency key and its `payloadHash` matches the payload. That integrity layer does
not prove that the named writer controlled the claimed actor identity.

Federated review workflows need an authenticity layer that survives at-rest storage and later
forwarding. Transport authentication alone is insufficient because the durable event may be
replayed after the connection that carried it is gone.

## Decision

Add optional per-event Ed25519 signatures to the `shore.event` envelope. Unsigned historical events
remain valid and continue to serialize without new fields. Signed events add two top-level envelope
siblings:

```json
{
  "signer": "did:key:z6Mk...",
  "signature": {
    "alg": "ed25519",
    "sigVersion": 1,
    "sig": "base64-ed25519-signature"
  }
}
```

`signature = { alg, sigVersion, sig }` is the complete v1 signature object. It does not carry
`publicKey` or `keyId`; the signing identity is the top-level `signer`, or, for self-certifying
events, the `writer.actorId` when that actor id is itself a `did:key`.

For `sigVersion = 1`, the payload type is:

```text
application/vnd.shore.event-tbs.v1+json
```

The signed bytes are literal Dead Simple Signing Envelope (DSSE) pre-authentication encoding over
the canonical `EventToBeSigned` JSON bytes. The media type is versioned and keeps `event-tbs.v1`
as the protocol media type label for "event to be signed"; public Rust names spell out
`EventToBeSigned`:

```text
payloadType = "application/vnd.shore.event-tbs.v1+json"
toBeSignedBytes = canonical_json(EventToBeSigned)
message         = preAuthenticationEncoding(payloadType, toBeSignedBytes)
signature       = Ed25519.sign(message)
```

The Dead Simple Signing Envelope pre-authentication encoding byte format is:

```text
preAuthenticationEncoding(type, body) = "DSSEv1" SP len(type) SP type SP len(body) SP body
```

`len` is the ASCII decimal byte length and `SP` is byte `0x20`.

## EventToBeSigned

`EventToBeSigned` is an explicit producer-fact view, not "the whole event minus signature." It
contains:

```text
{
  schema,
  version,
  eventType,
  eventId,
  payloadHash,
  target,
  actorId,
  signer,
  occurredAt,
  assertionMode
}
```

- `actorId` is `writer.actorId`, the claimed actor identity.
- `signer` is the resolved effective signer and is always a `did:key`.
- `payloadHash` binds the payload without signing raw payload bytes.
- `sourceRef` is excluded because it is hop metadata.
- `ingest` (the import-seam provenance stamp,
  [ADR-0009](./adr-0009-resumption-binding-trust-source.md)) is a realized instance of the
  hop-added metadata this exclusion anticipated; stamping a signed event cannot invalidate its
  signature.
- `sigVersion` is not inside the to-be-signed view; it selects the verifier path and payload type.

The to-be-signed view excludes `payload`, `sourceRef`, `ingest`, `signature`, `sigVersion`, and
future hop-added metadata.

## Identity And Trust

V1 uses `did:key:z6Mk...` for Ed25519 signer identity. A `did:key` identity may also be the
claimed `writer.actorId`.

`did:key` actor attribution and friendly `actor:*` attribution signed by the same key are distinct,
non-aliased identity claims. For example, `writer.actorId = did:key:P` and
`writer.actorId = actor:git-email:alice@example.com` with `signer = did:key:P` are different
events and remain distinct by design.

Verification resolves the effective signer as follows:

- If `signature` is present and `signer` is present, `signer` is the effective signer.
- If `signature` is present, `signer` is omitted, and `writer.actorId` is a `did:key`, that actor id
  is the effective signer.
- If `signature` is present and no effective signer can be resolved, verification is `invalid`.

Friendly `actor:*` ids are authorized by an allowed-signers trust set that maps actors to one or
more `did:key` signers. A self-certifying `did:key` actor is authorized only when the effective
signer is the same key.

## Verification Status And Policy

Verification returns one of these status values:

```text
valid / invalid / untrusted_key / unsigned
```

- `valid`: the signature verifies and the signer is trusted for the claimed actor.
- `invalid`: the key, algorithm, signature, version, or signed bytes are malformed or mismatched.
- `untrusted_key`: the signature verifies, but the signer is not authorized for the claimed actor.
- `unsigned`: the event has no signature.

Ed25519 verification uses strict semantics, such as `ed25519-dalek`'s `verify_strict` or an
equivalent normative ruleset. Unsupported algorithms, unsupported `sigVersion` values, malformed
`did:key` values, non-Ed25519 keys, truncated or over-long signatures, non-canonical public keys,
and signature mismatches are `invalid`.

Verification is advisory by default, matching ADR-0003. The policy presets are:

| Preset | `invalid` | `untrusted_key` | `unsigned` |
| ------ | --------- | --------------- | ---------- |
| `advisory` | accept with diagnostic | accept with diagnostic | accept with diagnostic |
| `integrity-strict` | reject | accept with diagnostic | accept with diagnostic |
| `trusted-strict` | reject | reject | reject unless `allowUnsigned` |

These presets separate corruption checks from trust-root enforcement and unsigned-event migration.
Verification status is separate from artifact availability: a valid signed event can still
reference an unavailable artifact.

## Idempotent Existing Events

Signatures do not select a conflict winner. When a write or ingest sees an already-stored event with
the same idempotency key and payload hash, the first stored event remains authoritative. If the later
copy has a different `signer` or `signature`, Shoreline keeps the first event and reports
`divergent_signature_existing_event` on ingest. Other metadata differences with the same payload hash
remain an idempotent existing event; a different payload under the same idempotency key remains a
conflict.

## Consequences

### Accepted

- Signatures authenticate durable events, not transport connections.
- The signed to-be-signed view binds event identity, payload hash, target, actor id, signer,
  timestamp, and assertion mode.
- Unsigned events remain valid so existing stores can be read and forwarded during migration.
- Advisory mode surfaces authenticity information without making trust a default write-side gate.
- Strict policies are explicit reader or ingest choices.
- `sourceRef` remains unsigned hop metadata and is not part of the producer signature.

### Rejected For V1

- A `.sig` sidecar, because it would split the event's forwarding unit.
- `publicKey` or `keyId` fields inside the v1 signature object.
- Persisting verification status in the event bytes.
- Using signatures to pick an automatic conflict winner.

## Deferred Vocabulary

These names are reserved for future work and are not implemented by this v1 signature contract.

### Signed Heads And Event-Set Roots

Signed track heads are deferred. When they exist, they will sign an `eventSetRoot` computed from a
versioned event-set algorithm. The reserved root algorithm name is:

```text
shore.event-set.canonical-map.v1
```

The reserved entry shape is:

```text
entry = eventId SP payloadHash SP eventRecordHash LF
eventSetRoot = sha256(concat(entries sorted by eventId, then eventRecordHash))
```

Reserved signed-head payload types:

```text
shore.trackHead.store-state.v1
shore.trackHead.producer-fact.v1
```

Reserved feature levels:

```text
none
trackRoot
parentAnchored
```

### Relay Attestation

`relay_attestation` is reserved as a future signed event family for durable relay provenance.
Per-event producer signatures do not authenticate who forwarded an event. `sourceRef` remains
unsigned hop metadata.

### Multi-Signature Envelopes

This v1 signature contract supports a single producer `signature`. Multi-signature event envelopes
are deferred. If a future design adopts `signatures: []`, signer identity belongs per signature
entry rather than as a single top-level `signer`.

## What Signatures Do Not Prove

Per-event signatures do not prove:

- global completeness;
- absence of selectively withheld events;
- confidentiality under selective replication;
- uncompromised human intent when a key holder or signing agent is compromised;
- relay provenance without a future `relay_attestation`;
- availability of referenced snapshot or note-body artifacts;
- an automatic winner for conflicting events.

## Future Work

Future review lineage and event-sync ADRs should cross-reference this ADR. New event families should
remain signable under the generic `EventToBeSigned` contract unless they intentionally introduce a
new `sigVersion` and payload type.

## Amendment: Detached Co-Signature Event Family

This amendment extends ADR-0004's deferred "Multi-Signature Envelopes" section and activates the
reserved `eventRecordHash` name into a concrete, **back-compatible** contract: signatures over a
Shoreline event form a **set of attestations** keyed to the event's signature-exclusive identity, and
multiple signatures over one fact are **co-signers, not a conflict**. The original decisions stand —
**Status:** stays Accepted; this is a landing record, not a re-decision. It introduces **no new
`sigVersion`** and **migrates no stored bytes**. It lands via shoreline plan 0068 (owner-approved
2026-06-17), the same `## Amendment` mechanism plan 0066 used for ADR-0010's "Key Custody Landing".
The governing definition of `eventRecordHash` lives in
[ADR-0008](./adr-0008-cross-peer-conflict-policy.md); the binding generalization it enables is the
amendment to [ADR-0009](./adr-0009-resumption-binding-trust-source.md); it composes under
[ADR-0010](./adr-0010-actor-identity-and-delegation.md) unchanged.

### Context

ADR-0004 v1 ships a **single** inline producer `signature` and explicitly defers multi-signature
envelopes ("If a future design adopts `signatures: []`, signer identity belongs per signature entry
rather than as a single top-level `signer`"). The field's settled cross-industry answer — DSSE
`signatures[]`, JWS, CMS SignerInfos, PGP, cosign + Rekor, Certificate Transparency — is uniform:
**identity is the content; signatures are a set of attestations attached to it.** The cautionary
tales (Bitcoin `txid` malleability; git's signature-in-the-SHA) are exactly why this amendment keeps
signatures *out* of the identity hash.

Shoreline's store is **append-only and content-addressed**: an event's stored bytes are immutable and
`eventId = sha256(idempotencyKey)` is already signature-exclusive, so you **cannot** grow an inline
`signatures: []` array on a stored event without rewriting its bytes. Co-signatures are therefore
forced into the only shape the substrate allows — **detached, append-only attestation records keyed
by content identity** — which is also the cosign/Rekor/PGP/git-notes pattern. And it is on-brand: *a
co-signature is itself an event.*

### Decision

#### D1 — The inline `signer`/`signature` is attestation #1

The v1 envelope `signer`/`signature` pair is reinterpreted, with **no byte change**, as the **first
member** of the event's co-signature set. An unsigned event has an empty set; a v1 single-signed event
has a one-member set. Nothing about already-stored events changes — a reinterpretation of existing
bytes, not a migration.

#### D2 — Additional attestations are a detached co-signature event family

Every attestation beyond the inline author signature is recorded as a member of a new **append-only
co-signature event family** (`event_signature`). A co-signature event is an ordinary `shore.event`: it
has its own `eventId`, `writer`, `occurredAt`, and replicates over the same event-sync plane as every
other event; it **references the target by its signature-exclusive content identity** —
`targetEventId` **and `targetEventRecordHash`** (the ADR-0008 signature-exclusive hash), **not**
`targetPayloadHash`; and its own `eventId`/idempotency key **derives from the full attestation
`(targetEventRecordHash, attestingSigner, signature)`**, so the member identity is the *whole triple*
(see D3), re-submitting the identical attestation is idempotent, and two distinct signatures by one
signer are two distinct members — never two claimants to one slot. Signer identity belongs per
attestation, never as a single top-level field.

#### D3 — Signatures do not enter event identity; the set converges by union

The target event's `eventId` and signature-exclusive `eventRecordHash` (ADR-0008) remain
**signature-exclusive**. The co-signature set is a **grow-only set (G-Set / join-semilattice)** whose
**member identity is the full attestation triple `(targetEventRecordHash, attestingSigner,
signature)`** — *not* `(targetEventRecordHash, signer)`. Keying on the full triple closes a
**signer-slot-poisoning** hazard: if member identity were `(target, signer)`, a malformed or
adversarial attestation occupying that slot first would, under first-wins idempotency, block the
signer's later valid attestation. With the full attestation in the identity, a valid attestation is a
*distinct* member from a bad one by the same signer; merge is set-union; identical triples dedup;
union is commutative, associative, and idempotent, so two stores holding different subsets of one
event's attestations **converge to the union with no winner-selection and no conflict**.

Because each co-signature is itself an *event*, **signature-set convergence is subsumed by event-set
convergence**: a store missing an attestation is missing that event and backfills it on the next sync.
Co-signature events carry their own `eventId`/`payloadHash` and are covered by the shipped
signature-blind `eventSetHash` and the reserved `eventSetRoot` like any event, while the *target's*
`eventRecordHash` stays signature-exclusive so a divergent inline author-signature never breaks root
convergence. There is **no separate signature-reconciliation channel** to build.

#### D4 — A co-signature attests the target's `EventToBeSigned` view (no new `sigVersion`)

The attestation in a co-signature event is an Ed25519 signature over the **target event's
`EventToBeSigned` view with `signer` set to the attesting signer** — the existing v1 message,
`application/vnd.shore.event-tbs.v1+json`, with the same DSSE pre-authentication encoding. **No new
`sigVersion`, no new payload type.** This is load-bearing twice: the inline author signature is
co-signature #1 **with no transformation** (D1), and a co-signature is verifiable with the unchanged
ADR-0004 verifier (strict Ed25519, allowed-signers authorization, the `valid / invalid /
untrusted_key / unsigned` status vocabulary, per attestation).

Two digests of the target are in play and **must never be confused**. The attestation signs the
**signer-inclusive** `EventToBeSigned` view (so each signer signs a view naming themselves and neither
attestation is replayable as the other), while the carrier binds the **signer-exclusive**
`targetEventRecordHash` — the convergent content-identity. These are **different digests over
different field sets** (the TBS view includes `signer`/`actorId` but not `payload`/`idempotencyKey`;
`eventRecordHash` includes `payload`/`idempotencyKey` but excludes `signer`/`signature`); they are
*not* interchangeable. A verifier reconstructs `EventToBeSigned` for the target with `signer` set to
the attestation's signer (all other fields from the target the carrier's `targetEventRecordHash`
resolves to) and checks the Ed25519 signature, so the co-signature is tied to exactly the
content-identity that converges across mirrors. The carrier event's own envelope provenance (who
*recorded* it, its ingest stamp) is **orthogonal** to the attestation's trust: a co-signature's trust
rests entirely on its embedded signature verifying against the trust set.

#### D5 — Verification is per-member; detached attestations verify before they store

The set's verification is the **multiset of per-attestation statuses**, and no member's status changes
another's — a `valid` attestation stands whatever else is in the set, which is what makes a fact's
trust robust to a single bad or revoked co-signer. A detached co-signature event **verifies
cryptographically before it is stored**: a structurally `invalid` one (the ADR-0004 `invalid` set) is
**rejected, not stored** (reader-independent noise), while `untrusted_key` is **kept** (reader-relative;
may become `valid` on a trust-set update). So the stored set contains only `valid` and `untrusted_key`
members. The **one** attestation that may be `invalid` in a stored event is the **inline** one — part
of the event's own bytes, kept per ADR-0004's "keep the event, surface `invalid`" rule and read only
by ADR-0009 arm (a).

#### D6 — Class-(b) divergence is reconciled by transcription, not reported as a conflict

When ingest offers an event whose `eventId`, `payloadHash`, **and signature-exclusive
`eventRecordHash`** match a stored event but whose inline attestation differs, the store keeps its
first-stored copy **and records the incoming inline attestation as a co-signature event** (D2),
converging the set to both signatures. The matching `eventRecordHash` is the precise predicate for
"this is the *same fact*, differently signed"; were `eventRecordHash` to differ, the copies are not the
same record and it is not a co-signature case. Because the incoming attestation is a real signature the
importer *received and can verify* over the target's TBS view, this is **transcription, not minting** —
the importer never needs the co-signer's private key and never forges anything (the relay never signs
as the reviewer); per D5 it transcribes only `valid`/`untrusted_key`, never `invalid`. The legacy
`divergent_signature_existing_event` signal is retired as a *divergence* report; a diagnostic now fires
only when the newly merged co-signer is **untrusted for the claimed actor**, not for divergence per se.

### Resolved design questions

| # | Question | Resolution |
| - | -------- | ---------- |
| 1 | Binding over a set: any-of vs threshold vs "responder's own signature present" | **Any-of a `valid` attestation.** ADR-0004 `valid` already means "verifies *and* signer authorized for the claimed `writer.actorId`," so any-of is intrinsically actor-scoped. Threshold-of-N (`require-k-cosigners`) is a named **deferred** policy tier. Detailed in the ADR-0009 amendment. |
| 2 | Storage shape; merge key; dedup | **New event family** (D2), not a sidecar. Merge is G-Set union with **member identity = the full attestation triple `(targetEventRecordHash, attestingSigner, signature)`** (D3); full-attestation keying + verify-before-store (D5) closes signer-slot poisoning. |
| 3 | Backward compatibility | **Inline `signer`/`signature` = attestation #1; no historical byte migration** (D1). Signature-exclusive identity is what makes this free. |
| 4 | Interaction with the trust lifecycle | Revoking one co-signer's key distrusts one *attestation*, never the fact's identity; a fact co-signed by A and B survives A's revocation on B's attestation (D5). Revocation/rotation/transparency over set members is designed separately. |
| 5 | `eventSetHash` / `eventSetRoot` | **Co-signature events are ordinary records in the set**, so `eventSetHash` (shipped, signature-blind) and the reserved `eventSetRoot` converge them as events; the *target's* `eventRecordHash` stays **signature-exclusive**. Signature-set reconciliation is therefore **not** a separate sync channel — it is event-set convergence. |

### Backward Compatibility

- **Already-stored single-signature events** are valid as written: their inline attestation is member
  #1 of a now-explicit set. No re-signing, no `eventId` change, no `sigVersion` change, golden vectors
  untouched.
- **Unsigned events** have an empty co-signature set and behave exactly as ADR-0004 specifies.
- **Mixed stores** are internally consistent; a reader without the co-signature events sees a smaller
  set and converges on backfill.
- **The v1 single-signer verifier** is a strict special case of the per-member verifier (a one-member
  set).

### Consequences

#### Accepted

- Multiple signatures over one fact are **co-signers, not a conflict**.
- Signatures are decoupled from identity: rotation is "co-sign with the new key," and a fact's trust is
  robust to single-key revocation.
- Conflict class (b) dissolves (ADR-0008); the relay's divergent-signature *report* becomes expected
  *reconciliation*.
- Binding generalizes to any-of a bound signer over the set (ADR-0009 amendment) without reopening
  either arm's trust basis.
- No `sigVersion` bump, no payload-type change, no historical byte migration.

#### Rejected

- **An inline `signatures: []` array on the event envelope** — impossible on a content-addressed,
  append-only store without rewriting stored bytes and breaking `eventId`.
- **A `.sig` sidecar** — splits the event's forwarding unit; detached *events* keep one forwarding unit
  and converge over the event plane.
- **Folding signatures into `eventId` / `eventRecordHash`** — re-affirmed rejected; it is what makes the
  divergent-signature conflict class exist in the first place.
- **The importer minting a co-signature on a reviewer's behalf** — transcription re-homes a received,
  verifiable signature; it never synthesizes one.
- **A dedicated co-signature payload type / new `sigVersion`** — breaks lossless transcription of a
  divergent inline attestation (D6) and adds a payload type for no convergence benefit.
