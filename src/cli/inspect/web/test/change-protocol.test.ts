import { describe, expect, it } from "vitest";
import type { ChangePresentation, ChangeSummary } from "../src/change-protocol";
import {
  buildChangePageUrl,
  buildEventHistoryUrl,
  CHANGE_READER_DOCUMENTS,
  decodeAuthorityCursorV2,
  decodeChangeDetail,
  decodeChangePage,
  decodeChangeRevisionDetail,
  decodeEventHistory,
  decodeReaderProfile,
  decodeRevisionInterdiff,
  decodeRevisionResource,
  requireCoherentGeneration,
  sameProfileGeneration,
  trimUnicodeWhitespace,
} from "../src/change-protocol";
import { authorityCursor } from "./support/authority";

const documents = CHANGE_READER_DOCUMENTS;

function profile(eventCount = 3) {
  return {
    schema: "pointbreak.inspect-reader-profile",
    version: 1,
    availability: "ready",
    authorityCursor: authorityCursor(eventCount),
    commitGraphStamp: "sha256:commit-graph",
    minimumReaderProfile: "review_change_revision_v1",
    documents,
  };
}

function capturedSnapshot(
  revision: { objectArtifactContentHash: string },
  objectId: string,
) {
  return {
    schema: "pointbreak.review-snapshot",
    version: 1,
    contentHash: revision.objectArtifactContentHash,
    snapshot: {
      review_id: "review:sha256:captured",
      object_id: objectId,
      files: [],
    },
  };
}

function availableResource(
  revision: {
    revisionId: string;
    objectArtifactContentHash: string;
  },
  objectId = "obj:sha256:target",
) {
  return {
    schema: "pointbreak.review-revision-resource",
    version: 1,
    projectionStamp: "sha256:generation",
    resource: { revision, objectId },
    projection: { includeBody: true },
    availability: "available",
    capturedDocumentHash: "sha256:captured",
    capturedDocument: capturedSnapshot(revision, objectId),
    diagnostics: [],
    cacheKey: "sha256:resource",
  };
}

function page(
  schema: "pointbreak.inspect-changes-page" | "pointbreak.inspect-attention",
  stamp = "sha256:generation",
) {
  return {
    schema,
    version: schema === "pointbreak.inspect-changes-page" ? 1 : 2,
    projectionStamp: stamp,
    diagnostics: [],
    next: "opaque-server-token",
    presentations: {
      "change:sha256:a": {
        currentRevisions: [],
      },
    },
    changes: [
      {
        changeId: "change:sha256:a",
        declarationState: "authoritative",
        titleAssertions: [],
        memberCount: 0,
        topology: "initial",
        lifecycle: "in_progress",
        attentionSummary: "in_progress",
        availabilitySummary: "available",
        currentRevisionRefs: [],
        projectionStamp: stamp,
      },
    ],
  };
}

function validEventHistoryValue() {
  return {
    schema: "pointbreak.inspect-event-history",
    version: 1,
    authorityCursor: authorityCursor(1),
    sourceChangeProjectionStamp: "sha256:generation",
    timelineProjectionStamp: "sha256:timeline",
    order: "desc",
    eventCount: 1,
    matchCount: 1,
    offset: 0,
    facets: { validation_check_recorded: 1 },
    completion: {
      eventTypes: ["validation_check_recorded"],
      trackIds: ["author"],
      changeIds: ["change:sha256:one"],
      revisionRefs: [],
      unresolvedRevisionIds: [],
    },
    diagnostics: [],
    queryNotices: [],
    entries: [
      {
        eventId: "evt:sha256:one",
        eventType: "validation_check_recorded",
        occurredAt: "2026-08-08T00:00:00Z",
        payloadHash: "sha256:payload",
        journalId: "journal:sha256:one",
        writer: {
          actorId: "actor:author",
          producer: { name: "pointbreak", version: "0.10.0" },
        },
        verificationStatus: "valid",
        assertionMode: "advisory",
        subject: {
          kind: "review",
          target: { kind: "revision", revisionId: "rev:sha256:one" },
        },
        changeIds: ["change:sha256:one"],
        revisionRefs: [],
        unresolvedRevisionIds: [],
        sourceRef: {
          sourceSystem: "legacy-review-journal",
          sourceId: "event:legacy:one",
        },
        ingest: {
          via: "ingest-events",
          receivedAt: "2026-08-08T00:00:01Z",
        },
        summary: {
          kind: "validation_check_recorded",
          details: {
            validationCheckId: "validation:sha256:one",
            target: { kind: "revision", revisionId: "rev:sha256:one" },
            checkName: "web",
            status: "passed",
            trigger: "manual",
          },
        },
      },
    ],
  };
}

describe("bounded Change protocol", () => {
  it("decodes only the complete closed authority cursor v2 shape", () => {
    const cursor = authorityCursor(3);
    const missingCapabilityHash: Record<string, unknown> = { ...cursor };
    delete missingCapabilityHash.capabilitySetHash;
    expect(decodeAuthorityCursorV2(cursor)).toEqual(cursor);

    for (const invalid of [
      { ...cursor, schema: "pointbreak.authority-cursor.v1" },
      { ...cursor, eventCount: -1 },
      { ...cursor, eventCount: cursor.journalRecordCount + 1 },
      { ...cursor, journalRecordCount: Number.MAX_SAFE_INTEGER + 1 },
      { ...cursor, eventSetHash: "" },
      { ...cursor, journalRecordSetHash: "sha256:abc" },
      {
        ...cursor,
        capabilitySetHash:
          "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
      },
      missingCapabilityHash,
      { ...cursor, futureGenerationField: "not admitted" },
    ]) {
      expect(() => decodeAuthorityCursorV2(invalid)).toThrow(
        "invalid authority cursor DTO",
      );
    }
  });

  it("constructs and validates the bounded Change-aware Timeline document", () => {
    expect(
      buildEventHistoryUrl({
        q: " Release ",
        type: "validation_check_recorded",
        order: "asc",
      }),
    ).toBe(
      "/api/v2/history?limit=100&q=release&type=validation_check_recorded&order=asc",
    );
    const document = decodeEventHistory(validEventHistoryValue());
    expect(document.entries[0]?.eventId).toBe("evt:sha256:one");
    expect(document.entries[0]?.verificationStatus).toBe("valid");
    expect(document.entries[0]?.sourceRef?.sourceSystem).toBe(
      "legacy-review-journal",
    );
    expect(() =>
      buildEventHistoryUrl({ at: "evt:one", after: "opaque" }),
    ).toThrow("mutually exclusive");
  });

  it("closes Timeline query and document event types", () => {
    expect(
      buildEventHistoryUrl({
        type: "validation_check_recorded,change_declared",
      }),
    ).toContain("type=change_declared%2Cvalidation_check_recorded");
    expect(() => buildEventHistoryUrl({ type: "foreign_event" })).toThrow(
      "unknown event type",
    );
    expect(() =>
      buildEventHistoryUrl({
        type: "validation_check_recorded,validation_check_recorded",
      }),
    ).toThrow("duplicate event type");

    const unknown = structuredClone(validEventHistoryValue());
    const unknownEvent = unknown.entries[0];
    if (!unknownEvent) throw new Error("fixture needs an event");
    unknownEvent.eventType = "foreign_event";
    expect(() => decodeEventHistory(unknown)).toThrow(
      "invalid event history DTO",
    );

    const mismatch = structuredClone(validEventHistoryValue());
    const mismatchEvent = mismatch.entries[0];
    if (!mismatchEvent) throw new Error("fixture needs an event");
    mismatchEvent.summary.kind = "change_declared";
    expect(() => decodeEventHistory(mismatch)).toThrow(
      "invalid event history DTO",
    );
  });

  it("rejects malformed event attribution and provenance", () => {
    const malformed = [
      (value: ReturnType<typeof validEventHistoryValue>) => {
        const event = value.entries[0];
        if (event) event.writer.actorId = "";
      },
      (value: ReturnType<typeof validEventHistoryValue>) => {
        const event = value.entries[0];
        if (event) event.assertionMode = "inferred";
      },
      (value: ReturnType<typeof validEventHistoryValue>) => {
        const event = value.entries[0];
        if (event) event.subject.kind = "task";
      },
      (value: ReturnType<typeof validEventHistoryValue>) => {
        const event = value.entries[0];
        if (event) event.sourceRef.sourceSystem = "";
      },
      (value: ReturnType<typeof validEventHistoryValue>) => {
        const event = value.entries[0];
        if (event) event.ingest.via = "filesystem-copy";
      },
    ];

    for (const corrupt of malformed) {
      const value = structuredClone(validEventHistoryValue());
      corrupt(value);
      expect(() => decodeEventHistory(value)).toThrow(
        "invalid event history DTO",
      );
    }
  });

  it("rejects an impossible Timeline loaded range", () => {
    const value = structuredClone(validEventHistoryValue());
    value.matchCount = 0;
    expect(() => decodeEventHistory(value)).toThrow(
      "invalid event history DTO",
    );
  });

  it("constructs canonical bounded URLs with the sole default order", () => {
    expect(buildChangePageUrl("changes")).toBe(
      "/api/v2/changes?limit=50&order=change_id_asc",
    );
    expect(
      buildChangePageUrl("attention", {
        q: "  Release Readiness  ",
        after: "opaque-token",
        limit: 100,
      }),
    ).toBe(
      "/api/v2/attention?limit=100&after=opaque-token&q=release+readiness&order=change_id_asc",
    );
  });

  it("decodes bounded pages without reordering server rows and preserves opaque next", () => {
    const decoded = decodeChangePage(page("pointbreak.inspect-changes-page"), {
      lens: "changes",
      bounded: true,
    });

    expect(decoded.changes.map((change) => change.changeId)).toEqual([
      "change:sha256:a",
    ]);
    expect(decoded.next).toBe("opaque-server-token");
  });

  it("rejects a bounded page that omits its explicit continuation member", () => {
    const response = page("pointbreak.inspect-attention");
    delete (response as { next?: string }).next;

    expect(() =>
      decodeChangePage(response, { lens: "attention", bounded: true }),
    ).toThrow("missing next");
  });

  it("uses the shared Unicode whitespace and lowercase normalization for q", () => {
    expect(
      buildChangePageUrl("changes", { q: "\u00a0RÉSUMÉ\u00a0" }),
    ).toContain("q=r%C3%A9sum%C3%A9");
    expect(trimUnicodeWhitespace("\u0085\u3000RÉSUMÉ\u00a0")).toBe("RÉSUMÉ");
  });

  it("enforces the query byte limit after Unicode lowercase normalization", () => {
    expect(() => buildChangePageUrl("changes", { q: "İ".repeat(100) })).toThrow(
      "at most 256 bytes",
    );
  });

  it("rejects malformed page DTOs and mixed projection stamps before paint", () => {
    expect(() =>
      decodeChangePage(
        { ...page("pointbreak.inspect-changes-page"), next: 3 },
        { lens: "changes", bounded: true },
      ),
    ).toThrow("next");

    const changes = decodeChangePage(page("pointbreak.inspect-changes-page"), {
      lens: "changes",
      bounded: true,
    });
    const attention = decodeChangePage(
      page("pointbreak.inspect-attention", "sha256:other"),
      { lens: "attention", bounded: true },
    );
    expect(() => requireCoherentGeneration(changes, attention)).toThrow(
      "coherent generation",
    );
  });

  it("requires the postflight capability and freshness profile to be unchanged", () => {
    const initial = decodeReaderProfile(profile());
    expect(sameProfileGeneration(initial, decodeReaderProfile(profile()))).toBe(
      true,
    );
    expect(
      sameProfileGeneration(initial, decodeReaderProfile(profile(4))),
    ).toBe(false);
    expect(
      sameProfileGeneration(
        initial,
        decodeReaderProfile({
          ...profile(),
          commitGraphStamp: "sha256:changed-commit-graph",
        }),
      ),
    ).toBe(false);
    expect(() =>
      decodeReaderProfile({ ...profile(), commitGraphStamp: "" }),
    ).toThrow("commit graph stamp");
    expect(() =>
      decodeReaderProfile({
        ...profile(),
        documents: { ...documents, "pointbreak.inspect-event-history": 1 },
      }),
    ).toThrow("incompatible Inspector reader profile");
  });

  it("rejects rows outside the projected domains, too-large pages, and foreign presentation revisions", () => {
    const invalidDomain = page("pointbreak.inspect-changes-page");
    const invalidDomainRow = invalidDomain.changes[0];
    if (!invalidDomainRow) throw new Error("fixture must include a Change row");
    invalidDomainRow.topology = "derived_in_browser";
    expect(() =>
      decodeChangePage(invalidDomain, { lens: "changes", bounded: true }),
    ).toThrow("invalid changes Change page DTO");

    const tooLarge = page("pointbreak.inspect-changes-page");
    const row = tooLarge.changes[0];
    if (!row) throw new Error("fixture must include a Change row");
    tooLarge.changes = Array.from({ length: 101 }, (_, index) => ({
      ...row,
      changeId: `change:sha256:${index}`,
    }));
    expect(() =>
      decodeChangePage(tooLarge, { lens: "changes", bounded: true }),
    ).toThrow("invalid changes Change page DTO");

    const foreignPresentation = page("pointbreak.inspect-changes-page");
    const presentation = foreignPresentation.presentations?.[
      "change:sha256:a"
    ] as ChangePresentation | undefined;
    if (!presentation) throw new Error("fixture must include a presentation");
    presentation.currentRevisions = [
      {
        revision: {
          revisionId: "rev:sha256:foreign",
          objectArtifactContentHash: "sha256:foreign",
        },
        summarySource: "absent",
      },
    ];
    expect(() =>
      decodeChangePage(foreignPresentation, { lens: "changes", bounded: true }),
    ).toThrow("invalid changes Change page DTO");
  });

  it("rejects unsorted server rows and inconsistent proposal-summary provenance", () => {
    const unsorted = page("pointbreak.inspect-changes-page");
    const row = unsorted.changes[0];
    if (!row) throw new Error("fixture must include a Change row");
    unsorted.changes = [
      { ...row, changeId: "change:sha256:b" },
      { ...row, changeId: "change:sha256:a" },
    ];
    expect(() =>
      decodeChangePage(unsorted, { lens: "changes", bounded: true }),
    ).toThrow("invalid changes Change page DTO");

    const inconsistent = page("pointbreak.inspect-changes-page");
    const presentation = inconsistent.presentations?.["change:sha256:a"] as
      | ChangePresentation
      | undefined;
    if (!presentation) throw new Error("fixture must include a presentation");
    presentation.currentRevisions = [
      {
        revision: {
          revisionId: "rev:sha256:a",
          objectArtifactContentHash: "sha256:a",
        },
        revisionProposalSummary: "present without a source",
        summarySource: "absent",
      },
    ];
    expect(() =>
      decodeChangePage(inconsistent, { lens: "changes", bounded: true }),
    ).toThrow("invalid changes Change page DTO");
  });

  it("requires a complete duplicate-free presentation bijection", () => {
    const missing = page("pointbreak.inspect-changes-page");
    const row = missing.changes[0];
    if (!row) throw new Error("fixture must include a Change row");
    missing.changes.push({ ...row, changeId: "change:sha256:b" });
    expect(() =>
      decodeChangePage(missing, { lens: "changes", bounded: true }),
    ).toThrow("invalid changes Change page DTO");

    const duplicate = page("pointbreak.inspect-changes-page");
    const revision = {
      revisionId: "rev:sha256:a",
      objectArtifactContentHash: "sha256:artifact-a",
    };
    const duplicateRow = (duplicate.changes as ChangeSummary[])[0];
    if (!duplicateRow) throw new Error("fixture must include a Change row");
    duplicateRow.currentRevisionRefs = [revision, revision];
    const duplicatePresentation = duplicate.presentations?.[
      "change:sha256:a"
    ] as ChangePresentation | undefined;
    if (!duplicatePresentation) {
      throw new Error("fixture must include a presentation");
    }
    duplicatePresentation.currentRevisions = [
      { revision, summarySource: "absent" },
      { revision, summarySource: "absent" },
    ];
    expect(() =>
      decodeChangePage(duplicate, { lens: "changes", bounded: true }),
    ).toThrow("invalid changes Change page DTO");
  });

  it("rejects malformed nested Change and exact-Revision DTOs", () => {
    const revision = {
      revisionId: "rev:sha256:a",
      objectArtifactContentHash: "sha256:artifact-a",
    };
    const summary = {
      ...page("pointbreak.inspect-changes-page").changes[0],
      currentRevisionRefs: [revision],
    };
    const changeDetail = {
      schema: "pointbreak.review-change",
      version: 1,
      summary,
      relationClaims: [
        {
          claimId: "claim:one",
          active: true,
          successor: revision,
          predecessor: null,
          supports: [],
          withdrawals: [],
        },
      ],
      diagnostics: [],
      projectionStamp: "sha256:generation",
    };
    expect(() => decodeChangeDetail(changeDetail)).toThrow("Change detail DTO");

    const revisionDetail = {
      schema: "pointbreak.review-change-revision",
      version: 1,
      changeId: "change:sha256:a",
      revision,
      revisionCurrency: "current",
      relationClassification: "current",
      availability: "available",
      factPresentations: [
        {
          factId: "fact:one",
          family: "observation",
          originRevision: null,
          revisionCurrency: "current",
          familyState: "current",
          availability: "available",
        },
      ],
      associations: [],
      diagnostics: [],
      projectionStamp: "sha256:generation",
    };
    expect(() => decodeChangeRevisionDetail(revisionDetail)).toThrow(
      "Revision detail DTO",
    );
    expect(() =>
      decodeRevisionResource({
        schema: "pointbreak.review-revision-resource",
        version: 1,
        resource: { revision: null, objectId: "obj:one" },
        availability: "available",
        diagnostics: [],
      }),
    ).toThrow("resource DTO");
    expect(() =>
      decodeRevisionInterdiff({
        schema: "pointbreak.review-revision-interdiff",
        version: 1,
        interdiff: { from: revision, to: null },
        availability: "unavailable",
        diagnostics: [],
      }),
    ).toThrow("interdiff DTO");
  });

  it("rejects a captured snapshot that is not bound to its exact resource", () => {
    const revision = {
      revisionId: "rev:sha256:target",
      objectArtifactContentHash: "sha256:target-artifact",
    };
    const resource = availableResource(revision);
    expect(decodeRevisionResource(resource).capturedDocument?.contentHash).toBe(
      revision.objectArtifactContentHash,
    );

    const wrongSchema = structuredClone(resource);
    wrongSchema.capturedDocument.schema = "pointbreak.review-revision";
    expect(() => decodeRevisionResource(wrongSchema)).toThrow("resource DTO");

    const wrongHash = structuredClone(resource);
    wrongHash.capturedDocument.contentHash = "sha256:other-artifact";
    expect(() => decodeRevisionResource(wrongHash)).toThrow("resource DTO");

    const wrongObject = structuredClone(resource);
    wrongObject.capturedDocument.snapshot.object_id = "obj:sha256:other";
    expect(() => decodeRevisionResource(wrongObject)).toThrow("resource DTO");
  });

  it("preserves every explicit fact-port carrier on contextual exact Revision detail", () => {
    const revision = {
      revisionId: "rev:sha256:target",
      objectArtifactContentHash: "sha256:target-artifact",
    };
    const origin = {
      revisionId: "rev:sha256:origin",
      objectArtifactContentHash: "sha256:origin-artifact",
    };
    const detail = {
      schema: "pointbreak.review-change-revision",
      version: 1,
      changeId: "change:sha256:one",
      revision,
      membershipSupport: [],
      revisionCurrency: "current",
      relationClassification: "current",
      availability: "available",
      exactRevisionDocument: availableResource(revision),
      factPresentations: [
        {
          factId: "obs:sha256:origin",
          family: "observation",
          originRevision: origin,
          contextChangeId: "change:sha256:one",
          presentedInRevision: revision,
          actorId: "actor:one",
          revisionCurrency: "current",
          familyState: "current",
          availability: "available",
        },
      ],
      factContentPresentations: {
        "obs:sha256:origin": {
          contentType: "text/markdown",
          bodyContentState: "present",
          content: {
            kind: "observation",
            title: "Ported observation",
            body: "Exact contextual body",
          },
        },
      },
      factPorts: [
        {
          portId: "fact-port:sha256:one",
          originRevision: origin,
          originFact: {
            kind: "observation",
            observationId: "obs:sha256:origin",
          },
          targetRevision: revision,
          relation: "carried_open_as",
          rationaleContentHash: "sha256:rationale",
          contextChangeId: "change:sha256:one",
          actorId: "actor:one",
          trackId: "track:review",
          sourceEventIds: ["evt:sha256:one"],
          applicability: "applicable",
          diagnostics: [],
        },
      ],
      associations: [],
      diagnostics: [],
      projectionStamp: "sha256:generation",
    };
    expect(decodeChangeRevisionDetail(detail).factPorts).toEqual(
      detail.factPorts,
    );
    const missingPorts = { ...detail };
    delete (missingPorts as { factPorts?: unknown }).factPorts;
    expect(() => decodeChangeRevisionDetail(missingPorts)).toThrow(
      "Revision detail DTO",
    );
  });

  it("rejects duplicate contextual fact identifiers before content joins", () => {
    const revision = {
      revisionId: "rev:sha256:target",
      objectArtifactContentHash: "sha256:target-artifact",
    };
    const fact = {
      factId: "obs:sha256:one",
      family: "observation",
      originRevision: revision,
      actorId: "actor:one",
      revisionCurrency: "current",
      familyState: "current",
      availability: "available",
    };
    const detail = {
      schema: "pointbreak.review-change-revision",
      version: 1,
      changeId: "change:sha256:one",
      revision,
      membershipSupport: [],
      revisionCurrency: "current",
      relationClassification: "current",
      availability: "available",
      exactRevisionDocument: availableResource(revision),
      factPresentations: [fact, { ...fact }],
      factContentPresentations: {
        [fact.factId]: {
          contentType: "text/plain",
          bodyContentState: "present",
          content: { kind: "observation", title: "One" },
        },
      },
      factPorts: [],
      associations: [],
      diagnostics: [],
      projectionStamp: "sha256:generation",
    };
    expect(() => decodeChangeRevisionDetail(detail)).toThrow(
      "Revision detail DTO",
    );
  });

  it("rejects fabricated or ambiguous applicable fact-port carriers", () => {
    const revision = {
      revisionId: "rev:sha256:target",
      objectArtifactContentHash: "sha256:target-artifact",
    };
    const origin = {
      revisionId: "rev:sha256:origin",
      objectArtifactContentHash: "sha256:origin-artifact",
    };
    const port = {
      portId: "fact-port:sha256:one",
      originRevision: origin,
      originFact: { kind: "observation", observationId: "obs:sha256:origin" },
      targetRevision: revision,
      relation: "carried_open_as",
      actorId: "actor:one",
      trackId: "track:review",
      sourceEventIds: ["evt:sha256:one"],
      applicability: "applicable",
      diagnostics: [],
    };
    const originFact = {
      factId: "obs:sha256:origin",
      family: "observation",
      originRevision: origin,
      presentedInRevision: revision,
      actorId: "actor:one",
      revisionCurrency: "current",
      familyState: "current",
      availability: "available",
    };
    const detail = {
      schema: "pointbreak.review-change-revision",
      version: 1,
      changeId: "change:sha256:one",
      revision,
      membershipSupport: [],
      revisionCurrency: "current",
      relationClassification: "current",
      availability: "available",
      exactRevisionDocument: availableResource(revision),
      factPresentations: [originFact],
      factContentPresentations: {
        [originFact.factId]: {
          contentType: "text/plain",
          bodyContentState: "present",
          content: { kind: "observation", title: "Ported" },
        },
      },
      factPorts: [port],
      associations: [],
      diagnostics: [],
      projectionStamp: "sha256:generation",
    };
    expect(decodeChangeRevisionDetail(detail).factPorts).toEqual([port]);

    const duplicatePort = structuredClone(detail);
    duplicatePort.factPorts.push({ ...port });
    expect(() => decodeChangeRevisionDetail(duplicatePort)).toThrow(
      "Revision detail DTO",
    );

    const emptyCarrier = structuredClone(detail);
    emptyCarrier.factPorts[0].sourceEventIds = [];
    expect(() => decodeChangeRevisionDetail(emptyCarrier)).toThrow(
      "Revision detail DTO",
    );

    const missingTrack = structuredClone(detail);
    const carrier = missingTrack.factPorts[0];
    if (!carrier) throw new Error("fixture needs a fact-port carrier");
    delete (carrier as { trackId?: string }).trackId;
    expect(() => decodeChangeRevisionDetail(missingTrack)).toThrow(
      "Revision detail DTO",
    );

    const missingOrigin = structuredClone(detail);
    missingOrigin.factPresentations = [];
    missingOrigin.factContentPresentations = {};
    expect(() => decodeChangeRevisionDetail(missingOrigin)).toThrow(
      "Revision detail DTO",
    );
  });
});
