import { describe, expect, it } from "vitest";
import {
  type Assessment,
  currentAssessmentSummary,
  factCard,
  factSection,
  type InputRequest,
  type Observation,
  type RevisionDetail,
  renderAssessmentCard,
  renderFactSupersessionBlock,
  renderInputRequestCard,
  renderObservationCard,
  renderValidationCheckCard,
  targetLabel,
  type ValidationCheck,
  verdictBadge,
} from "../src/cards";
import revisionJson from "./fixtures/revision.json";

function parse(html: string): Document {
  return new DOMParser().parseFromString(html, "text/html");
}

const detail = revisionJson as unknown as RevisionDetail & {
  observations: Observation[];
  inputRequests: InputRequest[];
  assessments: Assessment[];
  validationChecks: ValidationCheck[];
};

const observation = detail.observations[0];
const inputRequest = detail.inputRequests[0];
const currentAssessmentRecord = detail.assessments.find(
  (a) => a.status === "current",
) as Assessment;
const failedCheck = detail.validationChecks.find(
  (v) => v.status === "failed",
) as ValidationCheck;
const AUTHOR_ACTOR = "actor:agent:pointbreak-example-author";
const REVIEWER_ACTOR = "actor:agent:pointbreak-example-reviewer";
const authorWriter = {
  actorId: AUTHOR_ACTOR,
  producer: { name: "producer-must-not-replace-writer", version: "1.0.0" },
};
const reviewerWriter = {
  actorId: REVIEWER_ACTOR,
  producer: { name: "another-producer", version: "2.0.0" },
};

describe("verdictBadge", () => {
  it("renders the resolved assessment as the verdict value", () => {
    const badge = parse(verdictBadge(detail.currentAssessment)).querySelector(
      ".verdict",
    );
    expect(badge?.classList.contains("verdict-accepted")).toBe(true);
    expect(badge?.querySelector(".verdict-status")?.textContent).toBe(
      "current assessment",
    );
    expect(badge?.querySelector(".verdict-value")?.textContent).toBe(
      "accepted",
    );
  });

  it("counts candidates for an ambiguous verdict", () => {
    const badge = parse(
      verdictBadge({ status: "ambiguous", candidates: [{}, {}] }),
    ).querySelector(".verdict");
    expect(badge?.classList.contains("verdict-ambiguous")).toBe(true);
    expect(badge?.querySelector(".verdict-value")?.textContent).toBe(
      "ambiguous (2 candidates)",
    );
  });

  it("floors to unassessed for a missing or unresolved verdict", () => {
    for (const ca of [undefined, {}, { status: "unassessed" }]) {
      const badge = parse(verdictBadge(ca)).querySelector(".verdict");
      expect(badge?.classList.contains("verdict-unassessed")).toBe(true);
      expect(badge?.querySelector(".verdict-value")?.textContent).toBe(
        "unassessed",
      );
    }
  });
});

describe("currentAssessmentSummary", () => {
  it("renders the resolved assessment's summary body", () => {
    const summary = parse(currentAssessmentSummary(detail)).querySelector(
      ".verdict-summary",
    );
    expect(summary?.textContent).toContain("ship it");
  });

  it("renders a markdown summary when the content type selects it", () => {
    const md: RevisionDetail = {
      currentAssessment: { status: "resolved", assessmentId: "assess:md" },
      assessments: [
        {
          id: "assess:md",
          summary: "# Verdict",
          summaryContentType: "text/markdown",
        },
      ],
    };
    const summary = parse(currentAssessmentSummary(md)).querySelector(
      ".verdict-summary",
    );
    expect(summary?.classList.contains("markdown-body")).toBe(true);
    expect(summary?.querySelector("h1")?.textContent).toBe("Verdict");
  });

  it("counts unreplaced assessments for an ambiguous verdict", () => {
    const summary = parse(
      currentAssessmentSummary({
        currentAssessment: { status: "ambiguous", candidates: [{}, {}, {}] },
      }),
    ).querySelector(".verdict-summary");
    expect(summary?.textContent).toContain("3 unreplaced assessments");
  });

  it("renders nothing when there is no resolved summary", () => {
    expect(currentAssessmentSummary({ currentAssessment: {} })).toBe("");
  });

  it("mirrors the removed cue when the resolved summary was removed", () => {
    const removed: RevisionDetail = {
      currentAssessment: { status: "resolved", assessmentId: "assess:x" },
      assessments: [
        { id: "assess:x", summaryContentState: "physically_removed" },
      ],
    };
    const cue = parse(currentAssessmentSummary(removed)).querySelector(
      ".fact-body-removed",
    );
    expect(cue?.textContent).toBe("content removed");
  });

  it("also mirrors the cue for a suppressed-present removed summary", () => {
    const removed: RevisionDetail = {
      currentAssessment: { status: "resolved", assessmentId: "assess:s" },
      assessments: [
        { id: "assess:s", summaryContentState: "suppressed_present" },
      ],
    };
    expect(
      parse(currentAssessmentSummary(removed)).querySelector(
        ".fact-body-removed",
      ),
    ).toBeTruthy();
  });

  it("still renders nothing for a resolved assessment with neither summary nor removed state", () => {
    const plain: RevisionDetail = {
      currentAssessment: { status: "resolved", assessmentId: "assess:y" },
      assessments: [{ id: "assess:y" }],
    };
    expect(currentAssessmentSummary(plain)).toBe("");
  });
});

describe("targetLabel", () => {
  it("labels a range target with its file, span, and side", () => {
    expect(
      targetLabel({
        kind: "range",
        filePath: "src/lib.rs",
        startLine: 2,
        endLine: 4,
        side: "old",
      }),
    ).toBe("src/lib.rs:2-4 (old)");
  });

  it("defaults a range side to new and a single line span to itself", () => {
    expect(targetLabel({ kind: "range", filePath: "a.rs", startLine: 7 })).toBe(
      "a.rs:7-7 (new)",
    );
  });

  it("labels file, revision, and a cross-reference target", () => {
    expect(targetLabel({ kind: "file", filePath: "a.rs" })).toBe("a.rs");
    expect(targetLabel({ kind: "revision" })).toBe("whole revision");
    const doc = parse(
      targetLabel({ kind: "observation", observationId: "obs:sha256:abcdef" }),
    );
    expect(doc.querySelector(".ref")?.getAttribute("data-ref-kind")).toBe(
      "obs",
    );
  });

  it("falls back to the kind for an unknown or empty target", () => {
    expect(targetLabel({ kind: "weird" })).toBe("weird");
    expect(targetLabel(undefined)).toBe("");
  });
});

describe("factCard", () => {
  it("renders the shared card structure from its options", () => {
    const doc = parse(
      factCard("observation", {
        track: "agent:codex",
        title: "A title",
        status: "active",
        target: "src/lib.rs:2-2 (new)",
        tags: ["one", null, "two"],
        body: "the body",
        createdAt: "unix-ms:1782699185488",
        verify: '<span class="verify">v</span>',
        endorsements: '<div class="endorsements">e</div>',
        extra: '<div class="fact-rel">x</div>',
      }),
    );
    const card = doc.querySelector(".anno");
    expect(card?.classList.contains("anno-observation")).toBe(true);
    expect(doc.querySelector(".anno-kind-observation")?.textContent).toBe(
      "observation",
    );
    expect(doc.querySelector(".anno-track")?.textContent).toBe("agent:codex");
    expect(doc.querySelector(".anno-title")?.textContent).toContain("A title");
    const status = doc.querySelector(".fact-status");
    expect(status?.classList.contains("active")).toBe(true);
    expect(doc.querySelector(".anno-loc")?.textContent).toBe(
      "src/lib.rs:2-2 (new)",
    );
    // Falsy tags are dropped; the survivors render as badges.
    expect(
      Array.from(doc.querySelectorAll(".badge"), (b) => b.textContent),
    ).toEqual(["one", "two"]);
    expect(doc.querySelector(".anno-time")).not.toBeNull();
    expect(doc.querySelector(".verify")).not.toBeNull();
    expect(doc.querySelector(".endorsements")).not.toBeNull();
    expect(doc.querySelector(".fact-rel")?.textContent).toBe("x");
  });

  it("omits the optional cells when their options are absent", () => {
    const doc = parse(factCard("assessment", { title: "bare" }));
    expect(doc.querySelector(".fact-status")).toBeNull();
    expect(doc.querySelector(".anno-loc")).toBeNull();
    expect(doc.querySelector(".anno-time")).toBeNull();
  });
});

describe("renderObservationCard", () => {
  it("renders the fixture observation with its track, title, and target", () => {
    const doc = parse(renderObservationCard(observation));
    expect(doc.querySelector(".anno-observation")).not.toBeNull();
    expect(doc.querySelector(".anno-track")?.textContent).toBe("agent:codex");
    expect(doc.querySelector(".anno-title")?.textContent).toContain(
      "Observed change",
    );
    expect(doc.querySelector(".anno-loc")?.textContent).toBe(
      "src/lib.rs:2-2 (new)",
    );
    expect(doc.querySelector(".anno-body")?.textContent).toContain(
      "the return value changed",
    );
  });

  it("notes superseded observations in a relation row", () => {
    const doc = parse(
      renderObservationCard({
        ...observation,
        supersedes: ["obs:sha256:abc123def456"],
      }),
    );
    expect(doc.querySelector(".fact-rel")?.textContent).toContain("supersedes");
    expect(doc.querySelector(".fact-rel .ref")).not.toBeNull();
  });

  it("renders the exact writer separately from track and producer", () => {
    const doc = parse(
      renderObservationCard({
        ...observation,
        trackId: "agent:review-lane",
        writer: authorWriter,
      }),
    );
    const attribution = doc.querySelector(".anno-head");
    const actor = attribution?.querySelector<HTMLElement>(
      '[data-ref-kind="actor"]',
    );

    expect(attribution?.textContent).toContain("agent:review-lane");
    expect(attribution?.textContent).toContain("writer");
    expect(attribution?.textContent).not.toContain(
      "producer-must-not-replace-writer",
    );
    expect(actor?.textContent).toBe(AUTHOR_ACTOR);
    expect(actor?.dataset.refId).toBe(AUTHOR_ACTOR);
  });
});

describe("renderInputRequestCard", () => {
  it("renders the fixture input request with its mode/reason tags", () => {
    const doc = parse(renderInputRequestCard(inputRequest));
    expect(doc.querySelector(".anno-input-request")).not.toBeNull();
    expect(doc.querySelector(".anno-title")?.textContent).toContain(
      "Need a decision",
    );
    expect(doc.querySelector(".fact-status")?.textContent).toBe("open");
    const tags = Array.from(
      doc.querySelectorAll(".badge"),
      (b) => b.textContent,
    );
    expect(tags).toEqual(["operative", "manual_decision_required"]);
    expect(doc.querySelector(".anno-body")?.textContent).toContain(
      "should we ship this?",
    );
  });

  it("renders responses with their outcome and advisory readback", () => {
    const doc = parse(
      renderInputRequestCard({
        ...inputRequest,
        responses: [
          {
            outcome: "answered",
            reason: "go ahead",
            verificationStatus: "unsigned",
          },
        ],
      }),
    );
    const response = doc.querySelector(".fact-responses .fact-response");
    expect(response?.querySelector(".outcome")?.textContent).toBe("answered");
    expect(response?.textContent).toContain("go ahead");
    expect(response?.querySelector(".verify")).not.toBeNull();
  });

  it("keeps the opener and every ambiguous response writer and state visible in order", () => {
    const createdAt = ["unix-ms:1782699185700", "unix-ms:1782699185800"];
    const doc = parse(
      renderInputRequestCard({
        ...inputRequest,
        status: "ambiguous",
        writer: authorWriter,
        responses: [
          {
            id: "input-request-response:sha256:aaaaaaaaaaaaaaaa",
            outcome: "approved",
            reason: "ship this version",
            createdAt: createdAt[0],
            writer: reviewerWriter,
            verificationStatus: "valid",
            endorsements: [
              {
                classification: "endorsement-trusted",
                endorser: "actor:git-name:Reviewer One",
              },
            ],
          },
          {
            id: "input-request-response:sha256:bbbbbbbbbbbbbbbb",
            outcome: "rejected",
            reason: "wait for another pass",
            createdAt: createdAt[1],
            writer: authorWriter,
            verificationStatus: "unsigned",
          },
        ],
      }),
    );

    const opener = doc.querySelector<HTMLElement>(
      '.anno-input-request > .anno-head [data-ref-kind="actor"]',
    );
    expect(opener?.textContent).toBe(AUTHOR_ACTOR);
    expect(opener?.dataset.refId).toBe(AUTHOR_ACTOR);
    expect(doc.querySelector(".fact-status")?.textContent).toBe("ambiguous");

    const responses = Array.from(
      doc.querySelectorAll<HTMLElement>(".fact-response"),
    );
    expect(responses).toHaveLength(2);
    expect(
      responses.map(
        (response) =>
          response.querySelector<HTMLElement>('[data-ref-kind="actor"]')
            ?.dataset.refId,
      ),
    ).toEqual([REVIEWER_ACTOR, AUTHOR_ACTOR]);
    expect(responses[0].textContent).toContain("answered by");
    expect(responses[0].textContent).toContain("approved");
    expect(responses[0].textContent).toContain("ship this version");
    expect(responses[0].querySelector(".verify-valid")).not.toBeNull();
    expect(responses[0].querySelector(".endorsements")).not.toBeNull();
    expect(responses[1].textContent).toContain("rejected");
    expect(responses[1].textContent).toContain("wait for another pass");
    expect(
      responses.map((response) =>
        response.querySelector(".anno-time")?.getAttribute("title"),
      ),
    ).toEqual(createdAt);
  });
});

describe("renderAssessmentCard", () => {
  it("renders the current assessment with its display label and summary", () => {
    const doc = parse(renderAssessmentCard(currentAssessmentRecord));
    expect(doc.querySelector(".anno-assessment")).not.toBeNull();
    expect(doc.querySelector(".anno-title")?.textContent).toContain("accepted");
    expect(doc.querySelector(".fact-status")?.textContent).toBe("current");
    expect(doc.querySelector(".anno-body")?.textContent).toContain("ship it");
  });

  it("notes replaced and related facts in a relation row", () => {
    const doc = parse(
      renderAssessmentCard({
        ...currentAssessmentRecord,
        relatedObservations: ["obs:sha256:fedcba987654"],
      }),
    );
    const rel = doc.querySelector(".fact-rel")?.textContent ?? "";
    expect(rel).toContain("replaces");
    expect(rel).toContain("re ");
  });

  it("renders the assessment writer", () => {
    const doc = parse(
      renderAssessmentCard({
        ...currentAssessmentRecord,
        writer: reviewerWriter,
      }),
    );
    expect(
      doc.querySelector<HTMLElement>('[data-ref-kind="actor"]')?.dataset.refId,
    ).toBe(REVIEWER_ACTOR);
  });
});

describe("renderValidationCheckCard", () => {
  it("renders a validation check as an advisory card, not a verdict", () => {
    const doc = parse(renderValidationCheckCard(failedCheck));
    expect(doc.querySelector(".anno-validation")).not.toBeNull();
    expect(doc.querySelector(".anno-kind-validation")?.textContent).toBe(
      "validation",
    );
    expect(doc.querySelector(".anno-title")?.textContent).toContain(
      "cargo clippy",
    );
    // The status maps to .fact-status.<status>; failed never becomes a verdict.
    expect(
      doc.querySelector(".fact-status")?.classList.contains("failed"),
    ).toBe(true);
    const tags = Array.from(
      doc.querySelectorAll(".badge"),
      (b) => b.textContent,
    );
    expect(tags).toEqual(["manual", "exit 1"]);
    expect(doc.querySelector(".fact-rel")?.textContent).toContain(
      "cargo clippy -- -D warnings",
    );
  });

  it("renders the validation writer", () => {
    const doc = parse(
      renderValidationCheckCard({
        ...failedCheck,
        writer: reviewerWriter,
      }),
    );
    expect(
      doc.querySelector<HTMLElement>('[data-ref-kind="actor"]')?.textContent,
    ).toBe(REVIEWER_ACTOR);
  });

  it("keeps every raw status while rendering every server-projected disposition", () => {
    const cases = [
      ["passed", "current", "current result"],
      ["failed", "outstanding", "outstanding"],
      ["errored", "historical", "historical"],
      ["skipped", "skipped", "skipped"],
      ["failed", "resolved_by_later_pass", "resolved by strictly later pass"],
    ] as const;

    for (const [status, disposition, label] of cases) {
      const doc = parse(
        renderValidationCheckCard({ ...failedCheck, status }, disposition),
      );
      expect(
        doc.querySelector(".fact-status")?.classList.contains(status),
      ).toBe(true);
      expect(doc.querySelector(".validation-continuity")?.textContent).toBe(
        label,
      );
    }
  });

  it("shows a skipped fact as skipped while its group remains outstanding", () => {
    const doc = parse(
      renderValidationCheckCard(
        { ...failedCheck, status: "skipped" },
        "outstanding",
      ),
    );
    expect(doc.querySelector(".fact-status.skipped")?.textContent).toBe(
      "skipped",
    );
    expect(doc.querySelector(".validation-continuity")?.textContent).toBe(
      "outstanding",
    );
  });

  it("falls back to raw validation evidence when continuity is absent", () => {
    const doc = parse(renderValidationCheckCard(failedCheck));
    expect(doc.querySelector(".fact-status.failed")).not.toBeNull();
    expect(doc.querySelector(".validation-continuity")).toBeNull();
  });
});

describe("factSection", () => {
  it("renders a counted section over its items", () => {
    const doc = parse(
      factSection("Observations", [observation], renderObservationCard),
    );
    expect(doc.querySelector("h2")?.textContent).toBe("Observations (1)");
    expect(doc.querySelector(".anno-observation")).not.toBeNull();
  });

  it("renders a none placeholder and an optional context note", () => {
    const empty = parse(factSection("Observations", [], renderObservationCard));
    expect(empty.querySelector("h2")?.textContent).toBe("Observations (0)");
    expect(empty.querySelector(".up-empty")?.textContent).toBe("none");

    const withContext = parse(
      factSection(
        "Validation",
        [],
        renderValidationCheckCard,
        '<p class="validation-note">context only</p>',
      ),
    );
    expect(withContext.querySelector(".validation-note")?.textContent).toBe(
      "context only",
    );
  });
});

describe("advisory framing", () => {
  it("flows the reader-relative, never-gates copy through a card's readback", () => {
    const doc = parse(
      renderObservationCard({
        ...observation,
        verificationStatus: "valid",
        endorsements: [
          {
            classification: "endorsement-trusted",
            endorser: "actor:git-name:K",
          },
        ],
      }),
    );
    const verifyTitle =
      doc.querySelector(".verify")?.getAttribute("title") ?? "";
    expect(verifyTitle).toContain("reader-relative");
    expect(verifyTitle).toContain("never gates a write");
    const endorseTitle =
      doc.querySelector(".endorsements")?.getAttribute("title") ?? "";
    expect(endorseTitle).toContain("reader-relative");
    expect(endorseTitle).toContain("never gates a write");
  });
});

describe("renderFactSupersessionBlock (the fork-gated fact DAG figure)", () => {
  const graph = {
    laidOut: {
      nodes: [
        {
          id: "as:a",
          x: 30,
          y: 20,
          w: 50,
          h: 22,
          isHead: false,
          isSuperseded: true,
        },
        {
          id: "as:b",
          x: 20,
          y: 70,
          w: 50,
          h: 22,
          isHead: true,
          isSuperseded: false,
        },
        {
          id: "as:c",
          x: 90,
          y: 70,
          w: 50,
          h: 22,
          isHead: true,
          isSuperseded: false,
        },
      ],
      edges: [
        {
          from: "as:b",
          to: "as:a",
          path: [
            [20, 58],
            [30, 32],
          ],
          kind: "replaces",
        },
      ],
      bounds: { w: 150, h: 100 },
    },
  };

  it("wraps the painter in a labeled fact-dag figure with the fact nodes", () => {
    const html = renderFactSupersessionBlock(graph, "assessment");
    const doc = new DOMParser().parseFromString(html, "text/html");
    expect(doc.querySelector("figure.fact-dag")).not.toBeNull();
    expect(
      doc.querySelector("figure.fact-dag figcaption")?.textContent,
    ).toContain("assessment");
    // Reused painter: revision-dag svg root, data-fact-id nodes, 2 competing heads.
    expect(
      doc.querySelector("figure.fact-dag svg.revision-dag"),
    ).not.toBeNull();
    expect(doc.querySelectorAll("g.dag-node[data-fact-id]").length).toBe(3);
    expect(doc.querySelectorAll("g.dag-node.head").length).toBe(2);
    // Non-interactive.
    expect(doc.querySelector("g.dag-node[role]")).toBeNull();
  });

  it("returns '' when the graph is absent or empty", () => {
    expect(renderFactSupersessionBlock(undefined, "assessment")).toBe("");
    expect(
      renderFactSupersessionBlock(
        { laidOut: { nodes: [], edges: [], bounds: { w: 0, h: 0 } } },
        "observation",
      ),
    ).toBe("");
  });
});

describe("removed body cue", () => {
  it("renders the content-removed cue for a physically removed body", () => {
    const html = renderObservationCard({
      ...observation,
      body: undefined,
      bodyContentState: "physically_removed",
    });
    const doc = parse(html);
    const cue = doc.querySelector(".fact-body-removed");
    expect(cue?.textContent).toBe("content removed");
    expect(cue?.getAttribute("title")).toBe(
      "removed; bytes swept from the store",
    );
  });

  it("renders the suppressed-present cue with the still-stored title", () => {
    const html = renderObservationCard({
      ...observation,
      body: undefined,
      bodyContentState: "suppressed_present",
    });
    const doc = parse(html);
    const cue = doc.querySelector(".fact-body-removed");
    expect(cue?.textContent).toBe("content removed");
    expect(cue?.getAttribute("title")).toBe(
      "removal recorded; bytes still stored until compact",
    );
  });

  it("renders no cue without a removed state", () => {
    const html = renderObservationCard(observation);
    expect(parse(html).querySelector(".fact-body-removed")).toBeNull();
  });

  it("renders the cue for a removed response reason", () => {
    const html = renderInputRequestCard({
      ...inputRequest,
      responses: [
        {
          outcome: "approved",
          reason: undefined,
          reasonContentState: "physically_removed",
        },
      ],
    });
    expect(parse(html).querySelector(".fact-body-removed")).not.toBeNull();
  });
});
