import { describe, expect, it } from "vitest";
import {
  eventSubjectLabel,
  eventTypeColor,
  presentEvent,
} from "../src/change-inspector-event-presentation";
import type {
  EventHistoryEntry,
  EventHistorySummary,
} from "../src/change-protocol";

function event(summary: EventHistorySummary): EventHistoryEntry {
  return {
    eventId: `evt:sha256:${summary.kind}`,
    eventType: summary.kind,
    occurredAt: "2026-08-08T00:00:00Z",
    payloadHash: "sha256:payload",
    journalId: "journal:sha256:one",
    writer: {
      actorId: "actor:author",
      producer: { name: "pointbreak", version: "0.10.0" },
    },
    verificationStatus: "valid",
    assertionMode: "advisory",
    subject: { kind: "journal", journalId: "journal:sha256:one" },
    changeIds: [],
    revisionRefs: [],
    unresolvedRevisionIds: [],
    summary,
  };
}

describe("typed Timeline presentation", () => {
  it("presents Change declarations as readable stable-work identity", () => {
    const presentation = presentEvent(
      event({
        kind: "change_declared",
        details: {
          schema: "pointbreak.change-declared",
          version: 1,
          declarationClaimId: "change-declaration:sha256:one",
          changeId: "change:sha256:one",
          identityDescriptor: {
            kind: "root_revision",
            schema: "pointbreak.change-identity.v1",
            revision_id: "rev:sha256:root",
          },
          claimNonce: "nonce-one",
        },
      }),
    );

    expect(presentation.title).toBe("Declared change:sha256:one");
    expect(presentation.body).toContain("rev:sha256:root");
    expect(eventTypeColor("change_declared")).toBe("var(--evt-capture)");
  });

  it("makes relation corrections explicit instead of rendering raw JSON", () => {
    const presentation = presentEvent(
      event({
        kind: "change_revision_relation_withdrawn",
        details: {
          schema: "pointbreak.change-revision-relation-withdrawn",
          version: 1,
          relationWithdrawalId: "relation-withdrawal:sha256:one",
          relationClaimId: "relation:sha256:one",
          claimNonce: "nonce-two",
        },
      }),
    );

    expect(presentation.title).toBe(
      "Withdrew Revision-relation claim relation:sha256:one",
    );
    expect(presentation.fields).toContainEqual({
      label: "claim",
      value: "relation:sha256:one",
    });
  });

  it("titles Git ref withdrawals with the withdrawn association id", () => {
    const presentation = presentEvent(
      event({
        kind: "revision_ref_withdrawn",
        details: {
          refWithdrawalId: "ref-withdrawal:sha256:one",
          refAssociationId: "ref-assoc:sha256:aa11",
          target: { kind: "revision", revisionId: "rev:sha256:one" },
        },
      }),
    );

    expect(presentation.title).toBe(
      "Withdrew Git ref association ref-assoc:sha256:aa11",
    );
  });

  it("titles commit withdrawals with the withdrawn association id", () => {
    const presentation = presentEvent(
      event({
        kind: "revision_commit_withdrawn",
        details: {
          commitWithdrawalId: "commit-withdrawal:sha256:one",
          commitAssociationId: "commit-assoc:sha256:bb22",
          target: { kind: "revision", revisionId: "rev:sha256:one" },
        },
      }),
    );

    expect(presentation.title).toBe(
      "Withdrew commit association commit-assoc:sha256:bb22",
    );
  });

  it("titles membership withdrawals with the withdrawn claim id", () => {
    const presentation = presentEvent(
      event({
        kind: "change_membership_withdrawn",
        details: {
          schema: "pointbreak.change-membership-withdrawn",
          version: 1,
          membershipWithdrawalId: "membership-withdrawal:sha256:one",
          membershipClaimId: "membership:sha256:cc33",
          claimNonce: "nonce-three",
        },
      }),
    );

    expect(presentation.title).toBe(
      "Withdrew Change membership claim membership:sha256:cc33",
    );
  });

  it("keeps fact-port origin, target, relation, and Change context visible", () => {
    const presentation = presentEvent(
      event({
        kind: "review_fact_ported",
        details: {
          schema: "pointbreak.review-fact-ported",
          version: 1,
          portId: "port:sha256:one",
          originRevision: {
            revisionId: "rev:sha256:old",
            objectArtifactContentHash: "sha256:old-artifact",
          },
          originFact: {
            kind: "observation",
            observationId: "obs:sha256:one",
          },
          targetRevision: {
            revisionId: "rev:sha256:new",
            objectArtifactContentHash: "sha256:new-artifact",
          },
          relation: "context_only",
          targetFact: null,
          rationaleContentHash: "sha256:rationale",
          contextChangeId: "change:sha256:one",
        },
      }),
    );

    expect(presentation.title).toContain("context only observation");
    expect(presentation.body).toContain("rev:sha256:old");
    expect(presentation.body).toContain("rev:sha256:new");
    expect(presentation.fields).toContainEqual({
      label: "Change context",
      value: "change:sha256:one",
    });
    expect(
      eventSubjectLabel({
        kind: "review_fact_port",
        portId: "port:sha256:one",
        originRevision: {
          revisionId: "rev:sha256:old",
          objectArtifactContentHash: "sha256:old-artifact",
        },
        originFact: {
          kind: "observation",
          observationId: "obs:sha256:one",
        },
      }),
    ).toContain("Fact port port:sha256:one");
  });
});
