import { describe, expect, it } from "vitest";
import {
  type Assessment,
  currentAssessmentSummary,
  factCard,
  factSection,
  type InputRequest,
  type Observation,
  type RevisionDetail,
  renderAdapterNoteCard,
  renderAssessmentCard,
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
});

describe("renderAdapterNoteCard", () => {
  it("renders an imported note with its author and file target", () => {
    const doc = parse(
      renderAdapterNoteCard({
        title: "imported note",
        filePath: "src/lib.rs",
        body: "from another tool",
      }),
    );
    expect(doc.querySelector(".anno-observation")).not.toBeNull();
    expect(doc.querySelector(".anno-track")?.textContent).toBe("imported");
    expect(doc.querySelector(".anno-loc")?.textContent).toBe("src/lib.rs");
  });

  it("uses the note author as the track when present", () => {
    const doc = parse(
      renderAdapterNoteCard({ title: "n", author: "linter-bot" }),
    );
    expect(doc.querySelector(".anno-track")?.textContent).toBe("linter-bot");
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
