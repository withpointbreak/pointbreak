import { describe, expect, it } from "vitest";
import type { ChangePresentation, ChangeSummary } from "../src/change-protocol";
import {
  buildChangePageUrl,
  CHANGE_READER_DOCUMENTS,
  decodeChangeDetail,
  decodeChangePage,
  decodeChangeRevisionDetail,
  decodeReaderProfile,
  decodeRevisionInterdiff,
  decodeRevisionResource,
  requireCoherentGeneration,
  sameProfileGeneration,
  trimUnicodeWhitespace,
} from "../src/change-protocol";

const documents = CHANGE_READER_DOCUMENTS;

function profile(eventCount = 3) {
  return {
    schema: "pointbreak.inspect-reader-profile",
    version: 1,
    availability: "ready",
    authorityCursor: { eventCount, eventSetHash: "sha256:authority" },
    commitGraphStamp: "sha256:commit-graph",
    minimumReaderProfile: "review_change_revision_v1",
    documents,
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

describe("bounded Change protocol", () => {
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
});
