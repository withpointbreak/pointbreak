# Signing UX

Shoreline events may carry an optional Ed25519 signature that authenticates the producer facts. This
page orients you across the three signing flows and the verification ladder. Reference material lives
in [cli-reference.md](./cli-reference.md) (the `shore keys` family and signing env vars),
[storage-model.md](./storage-model.md) (the allowed-signers format and the user-level key home), and
[ADR-0010](./adr/adr-0010-actor-identity-and-delegation.md) (the decisions).

## Signing never gates a write

This is the load-bearing rule: **a write never fails because of signing.** Whatever goes wrong while
resolving a key — none configured, an unreadable key home, an unsupported algorithm, a malformed
configured key, or `SHORE_SIGNING=off` — degrades to an **unsigned write at exit 0**, with a one-line
advisory diagnostic on stderr. Signing strengthens a write when it can; it never blocks one.

## The verification ladder

A read surface (with a verification policy and the discovered trust set) renders one of:

- **`unsigned`** — no signature: no key was configured, `SHORE_SIGNING=off`, or keygen failed. The
  write still happened.
- **`untrusted_key`** — signed by a key that is not in the repo's `.shore/allowed-signers.json`
  allow-list. Tamper-evident and strictly better than unsigned, but not yet bound to an actor.
- **`valid`** — signed by a key enrolled for that actor (or a self-certifying `did:key` actor whose
  id is its own signer). The signature verifies and binds.

(`invalid` is the fourth status — a signature that fails to verify against its claimed key.)

## Three flows

### Human: `init` then `enroll`

```bash
shore keys init --name default          # generate a key, print its did:key
shore keys enroll default --actor actor:git-email:alice@example.com
git add .shore/allowed-signers.json && git commit   # the commit is the authorization
```

The human opts in explicitly. Until the enrollment is committed, the human's signed events render
`untrusted_key`; once committed, they render `valid`.

### Agent: auto-keygen on first write

An agent writing under an `actor:agent:*` id needs no setup. The first write silently generates a
passphrase-less per-machine key, signs, and prints a notice with the agent's `did:key` and
`shore keys enroll`. A human reviews and commits the allow-list edit to bind the agent. See
[agent-authoring.md](./agent-authoring.md). `SHORE_SIGNING=off` opts out.

### CI: ephemeral self-certifying `did:key`

CI can sign without any enrollment by making the writing actor *be* the signing key:

```bash
shore keys init --name ci
export SHORE_ACTOR_ID="$(shore keys show ci --did | jq -r .didKey)"
shore review capture --sign-key ci   # writer.actorId == signer -> self-certifying
```

Because `writer.actorId` is the signing key's `did:key`, the event omits the top-level `signer` and
verifies `valid` under an empty trust set — no allow-list entry required. The key is ephemeral to the
CI run.

## Deferred

`shore keys use-ssh` (reuse an existing SSH key through ssh-agent), key rotation, and revocation are
named follow-ons, not yet shipped — only the flows above are available today.
