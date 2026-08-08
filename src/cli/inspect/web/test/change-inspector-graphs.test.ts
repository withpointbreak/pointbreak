import { describe, expect, it, vi } from "vitest";
import {
  renderChangeRevisionGraph,
  renderFactRelationshipGraph,
} from "../src/change-inspector-graphs";
import type {
  ChangeRevisionGraphPresentation,
  FactRelationshipGraphPresentation,
  RevisionRef,
} from "../src/change-protocol";

const first: RevisionRef = {
  revisionId: `rev:sha256:${"1".repeat(64)}`,
  objectArtifactContentHash: `sha256:${"a".repeat(64)}`,
};
const second: RevisionRef = {
  revisionId: `rev:sha256:${"2".repeat(64)}`,
  objectArtifactContentHash: `sha256:${"b".repeat(64)}`,
};
const peer: RevisionRef = {
  revisionId: `rev:sha256:${"3".repeat(64)}`,
  objectArtifactContentHash: `sha256:${"c".repeat(64)}`,
};
const context: RevisionRef = {
  revisionId: `rev:sha256:${"4".repeat(64)}`,
  objectArtifactContentHash: `sha256:${"d".repeat(64)}`,
};

function revisionKey(revision: RevisionRef): string {
  return `revision:${revision.revisionId}@${revision.objectArtifactContentHash}`;
}

function factKey(
  revision: RevisionRef,
  family: string,
  factId: string,
): string {
  return `fact:${revision.revisionId}@${revision.objectArtifactContentHash}:${family}:${factId}`;
}

function changeGraph(): ChangeRevisionGraphPresentation {
  return {
    nodes: [
      {
        id: revisionKey(peer),
        revision: peer,
        x: 260,
        y: 40,
        w: 128,
        h: 34,
        isCurrent: true,
        isMember: true,
        contextAvailability: "available",
        activationRevision: peer,
      },
      {
        id: revisionKey(first),
        revision: first,
        x: 150,
        y: 150,
        w: 128,
        h: 34,
        isCurrent: false,
        isMember: true,
        contextAvailability: "available",
        activationRevision: first,
      },
      {
        id: revisionKey(second),
        revision: second,
        x: 40,
        y: 40,
        w: 128,
        h: 34,
        isCurrent: true,
        isMember: true,
        contextAvailability: "available",
        activationRevision: second,
      },
      {
        id: revisionKey(context),
        revision: context,
        x: 370,
        y: 150,
        w: 128,
        h: 34,
        isCurrent: false,
        isMember: false,
        contextAvailability: "relationship_context_only",
      },
    ],
    effectiveSupersedes: [
      {
        from: revisionKey(second),
        to: revisionKey(first),
        successor: second,
        predecessor: first,
        path: [
          [150, 150],
          [40, 40],
        ],
      },
    ],
    pendingOrConflictingClaims: [
      {
        claimId: `change-relation:sha256:${"d".repeat(64)}`,
        from: revisionKey(context),
        to: revisionKey(first),
        successor: context,
        predecessor: first,
        path: [
          [150, 150],
          [370, 150],
        ],
        diagnostics: ["claim has conflicting support"],
      },
    ],
    bounds: { w: 440, h: 190 },
    diagnostics: ["Change relation state is conflicted"],
  };
}

describe("Change Revision graph renderer", () => {
  it("keeps effective state distinct from claims and exposes a readable equivalent", () => {
    const graph = changeGraph();
    const activate = vi.fn();
    const figure = renderChangeRevisionGraph(graph, {
      document,
      onActivateRevision: activate,
    });

    const effective = figure.querySelector<SVGGElement>(
      '[data-edge-kind="effective-supersedes"]',
    );
    const claim = figure.querySelector<SVGGElement>(
      '[data-edge-kind="pending-or-conflicting-claim"]',
    );
    expect(
      effective?.querySelector("polyline")?.getAttribute("stroke-dasharray"),
    ).toBe("none");
    expect(
      claim?.querySelector("polyline")?.getAttribute("stroke-dasharray"),
    ).toBe("7 5");
    expect(effective?.getAttribute("aria-label")).toContain(
      first.objectArtifactContentHash,
    );
    expect(claim?.getAttribute("aria-label")).toContain(
      "Pending or conflicting supersedes claim",
    );
    expect(claim?.getAttribute("aria-label")).toContain(
      "claim has conflicting support",
    );

    const equivalent = figure.querySelector<HTMLElement>(
      "[data-graph-textual-equivalent]",
    );
    expect(equivalent?.textContent).toContain(
      "Effective supersedes relationship",
    );
    expect(equivalent?.textContent).toContain(
      "Pending or conflicting supersedes claim",
    );
    expect(equivalent?.textContent).toContain(
      "Graph diagnostic: Change relation state is conflicted",
    );
  });

  it("makes every current peer an independent exact activation target", () => {
    const activate = vi.fn();
    const figure = renderChangeRevisionGraph(changeGraph(), {
      document,
      onActivateRevision: activate,
    });
    const current = Array.from(
      figure.querySelectorAll<SVGGElement>(
        '.change-revision-node[data-current="true"]',
      ),
    );
    expect(current).toHaveLength(2);
    expect(current.map((node) => node.dataset.revisionId).sort()).toEqual(
      [second.revisionId, peer.revisionId].sort(),
    );

    current
      .find((node) => node.dataset.revisionId === peer.revisionId)
      ?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(activate).toHaveBeenLastCalledWith(peer);
    expect(activate).toHaveBeenCalledTimes(1);

    current
      .find((node) => node.dataset.revisionId === second.revisionId)
      ?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    expect(activate).toHaveBeenLastCalledWith(second);
    expect(activate).toHaveBeenCalledTimes(2);
  });

  it("keeps claim-only Revision context readable without promising a dead route", () => {
    const activate = vi.fn();
    const figure = renderChangeRevisionGraph(changeGraph(), {
      document,
      onActivateRevision: activate,
    });
    const node = figure.querySelector<SVGGElement>(
      `.change-revision-node[data-revision-id="${context.revisionId}"]`,
    );
    expect(node?.getAttribute("role")).toBe("group");
    expect(node?.hasAttribute("tabindex")).toBe(false);
    expect(node?.getAttribute("aria-disabled")).toBe("true");
    expect(node?.getAttribute("aria-label")).toContain(
      "relationship context only",
    );
    node?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    node?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    expect(activate).not.toHaveBeenCalled();
    expect(
      figure.querySelector("[data-graph-textual-equivalent]")?.textContent,
    ).toContain(context.objectArtifactContentHash);
  });

  it("names textual actions as actions while preserving exact graph identity", () => {
    const figure = renderChangeRevisionGraph(changeGraph(), {
      document,
      onActivateRevision: vi.fn(),
    });
    const node = figure.querySelector<SVGGElement>(
      `.change-revision-node[data-revision-id="${second.revisionId}"]`,
    );
    const nodeName = node?.getAttribute("aria-label");
    const action = Array.from(
      figure.querySelectorAll<HTMLButtonElement>(
        "[data-graph-text-nodes] button",
      ),
    ).find((button) => button.title.includes(second.revisionId));
    expect(action?.title).toBe(`Open ${nodeName}`);
    expect(action?.getAttribute("aria-label")).toBe(`Open ${nodeName}`);

    const contextNode = figure.querySelector<SVGGElement>(
      `.change-revision-node[data-revision-id="${context.revisionId}"]`,
    );
    const contextItem = Array.from(
      figure.querySelectorAll<HTMLLIElement>("[data-graph-text-nodes] > li"),
    ).find(
      (item) => item.textContent === contextNode?.getAttribute("aria-label"),
    );
    expect(contextItem?.querySelector("button")).toBeNull();
  });

  it("shortens visible labels while retaining full exact identity everywhere semantic", () => {
    const figure = renderChangeRevisionGraph(changeGraph(), {
      document,
      onActivateRevision: vi.fn(),
    });
    const node = figure.querySelector<SVGGElement>(
      `.change-revision-node[data-revision-id="${second.revisionId}"]`,
    );
    expect(node?.querySelector("text")?.textContent).toBe(
      "current · rev:22222222",
    );
    expect(node?.querySelector("text")?.textContent).not.toContain(
      second.objectArtifactContentHash,
    );
    expect(node?.getAttribute("title")).toContain(second.revisionId);
    expect(node?.getAttribute("aria-label")).toContain(
      second.objectArtifactContentHash,
    );
    expect(node?.dataset.revisionId).toBe(second.revisionId);
    expect(node?.dataset.artifactHash).toBe(second.objectArtifactContentHash);
  });

  it("is deterministic even when DTO array order changes", () => {
    const graph = changeGraph();
    const reversed = {
      ...graph,
      nodes: [...graph.nodes].reverse(),
      effectiveSupersedes: [...graph.effectiveSupersedes].reverse(),
      pendingOrConflictingClaims: [
        ...graph.pendingOrConflictingClaims,
      ].reverse(),
    };
    const options = { document, onActivateRevision: vi.fn() };
    expect(renderChangeRevisionGraph(graph, options).outerHTML).toBe(
      renderChangeRevisionGraph(reversed, options).outerHTML,
    );
  });
});

const oldObservation = `observation:sha256:${"4".repeat(64)}`;
const newObservation = `observation:sha256:${"5".repeat(64)}`;
const oldAssessment = `assessment:sha256:${"6".repeat(64)}`;
const newAssessment = `assessment:sha256:${"7".repeat(64)}`;

function factGraph(): FactRelationshipGraphPresentation {
  return {
    nodes: [
      {
        id: factKey(first, "observation", oldObservation),
        kind: "fact",
        revision: first,
        factId: oldObservation,
        family: "observation",
        x: 60,
        y: 150,
        w: 150,
        h: 34,
        contextAvailability: "available",
        activationRevision: first,
      },
      {
        id: factKey(first, "observation", newObservation),
        kind: "fact",
        revision: first,
        factId: newObservation,
        family: "observation",
        x: 60,
        y: 40,
        w: 150,
        h: 34,
        contextAvailability: "available",
        activationRevision: first,
      },
      {
        id: factKey(first, "assessment", oldAssessment),
        kind: "fact",
        revision: first,
        factId: oldAssessment,
        family: "assessment",
        x: 240,
        y: 150,
        w: 150,
        h: 34,
        contextAvailability: "available",
        activationRevision: first,
      },
      {
        id: factKey(first, "assessment", newAssessment),
        kind: "fact",
        revision: first,
        factId: newAssessment,
        family: "assessment",
        x: 240,
        y: 40,
        w: 150,
        h: 34,
        contextAvailability: "available",
        activationRevision: first,
      },
      {
        id: revisionKey(second),
        kind: "revision",
        revision: second,
        x: 400,
        y: 150,
        w: 150,
        h: 34,
        contextAvailability: "available",
        activationRevision: second,
      },
    ],
    observationSupersedes: [
      {
        from: factKey(first, "observation", newObservation),
        to: factKey(first, "observation", oldObservation),
        originRevision: first,
        fromFactId: newObservation,
        toFactId: oldObservation,
        path: [
          [60, 150],
          [60, 40],
        ],
      },
    ],
    assessmentReplaces: [
      {
        from: factKey(first, "assessment", newAssessment),
        to: factKey(first, "assessment", oldAssessment),
        originRevision: first,
        fromFactId: newAssessment,
        toFactId: oldAssessment,
        path: [
          [240, 150],
          [240, 40],
        ],
      },
    ],
    factPorts: [
      {
        portId: `fact-port:sha256:${"8".repeat(64)}`,
        from: factKey(first, "observation", newObservation),
        to: revisionKey(second),
        originRevision: first,
        originFact: {
          kind: "observation",
          observationId: newObservation,
        },
        targetRevision: second,
        relation: "context_only",
        applicability: "conflicted",
        path: [
          [400, 150],
          [60, 40],
        ],
        diagnostics: ["target fact is deliberately absent"],
      },
    ],
    bounds: { w: 490, h: 190 },
  };
}

describe("exact fact graph renderer", () => {
  it("keeps observation, assessment, and fact-port relations distinct", () => {
    const figure = renderFactRelationshipGraph(factGraph(), {
      document,
      onActivateRevision: vi.fn(),
      onFocusFact: vi.fn(),
    });
    const observation = figure.querySelector<SVGGElement>(
      '[data-edge-kind="observation-supersedes"]',
    );
    const assessment = figure.querySelector<SVGGElement>(
      '[data-edge-kind="assessment-replaces"]',
    );
    const port = figure.querySelector<SVGGElement>(
      '[data-edge-kind="fact-port"]',
    );
    expect(
      observation?.querySelector("polyline")?.getAttribute("stroke-dasharray"),
    ).toBe("none");
    expect(
      assessment?.querySelector("polyline")?.getAttribute("stroke-dasharray"),
    ).toBe("10 4");
    expect(
      port?.querySelector("polyline")?.getAttribute("stroke-dasharray"),
    ).toBe("2 4");
    expect(observation?.getAttribute("aria-label")).toContain(
      "Observation supersedes relationship",
    );
    expect(assessment?.getAttribute("aria-label")).toContain(
      "Assessment replaces relationship",
    );
    expect(port?.getAttribute("aria-label")).toContain(
      "applicability conflicted",
    );
    expect(port?.getAttribute("aria-label")).toContain(
      "target fact is deliberately absent",
    );
    expect(
      figure.querySelector("[data-graph-textual-equivalent]")?.textContent,
    ).toContain("Fact port");
  });

  it("passes full exact fact focus and exact Revision anchor identities to callbacks", () => {
    const focusFact = vi.fn();
    const activateRevision = vi.fn();
    const figure = renderFactRelationshipGraph(factGraph(), {
      document,
      onActivateRevision: activateRevision,
      onFocusFact: focusFact,
    });
    const fact = figure.querySelector<SVGGElement>(
      `[data-graph-fact-id="${newObservation}"]`,
    );
    expect(fact?.getAttribute("title")).toContain(newObservation);
    expect(fact?.getAttribute("aria-label")).toContain(
      first.objectArtifactContentHash,
    );
    expect(fact?.dataset.graphFactFocus).toBe(
      JSON.stringify({
        revisionId: first.revisionId,
        objectArtifactContentHash: first.objectArtifactContentHash,
        family: "observation",
        factId: newObservation,
      }),
    );
    fact?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(focusFact).toHaveBeenCalledWith({
      revision: first,
      family: "observation",
      factId: newObservation,
    });
    expect(activateRevision).not.toHaveBeenCalled();

    figure
      .querySelector<SVGGElement>(".fact-relationship-node-revision")
      ?.dispatchEvent(
        new KeyboardEvent("keydown", { key: " ", bubbles: true }),
      );
    expect(activateRevision).toHaveBeenCalledWith(second);
  });

  it("shortens only the visible node label", () => {
    const figure = renderFactRelationshipGraph(factGraph(), {
      document,
      onActivateRevision: vi.fn(),
      onFocusFact: vi.fn(),
    });
    const node = figure.querySelector<SVGGElement>(
      `[data-graph-fact-id="${newAssessment}"]`,
    );
    expect(node?.querySelector("text")?.textContent).toBe(
      "assessment · assessment:77777777",
    );
    expect(node?.querySelector("text")?.textContent).not.toContain(
      first.objectArtifactContentHash,
    );
    expect(node?.dataset.revisionId).toBe(first.revisionId);
    expect(node?.dataset.artifactHash).toBe(first.objectArtifactContentHash);
  });

  it("renders unmaterialized fact-port context without activation", () => {
    const graph = factGraph();
    const contextual = graph.nodes.find(
      (node) => node.kind === "fact" && node.factId === oldObservation,
    );
    if (!contextual) throw new Error("fixture needs contextual fact");
    contextual.contextAvailability = "relationship_context_only";
    delete contextual.activationRevision;
    const focusFact = vi.fn();
    const figure = renderFactRelationshipGraph(graph, {
      document,
      onActivateRevision: vi.fn(),
      onFocusFact: focusFact,
    });
    const node = figure.querySelector<SVGGElement>(
      `[data-graph-fact-id="${oldObservation}"]`,
    );
    expect(node?.getAttribute("role")).toBe("group");
    expect(node?.hasAttribute("tabindex")).toBe(false);
    expect(node?.getAttribute("aria-disabled")).toBe("true");
    expect(node?.getAttribute("aria-label")).toContain(
      "relationship context only",
    );
    node?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(focusFact).not.toHaveBeenCalled();
  });
});
