import { readFileSync } from "node:fs";
import { resolve } from "node:path";
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

const APP_CSS = readFileSync(
  resolve(process.cwd(), "../assets/app.css"),
  "utf8",
);

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
        displayLabel: "current · rev:33333333",
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
        displayLabel: "rev:11111111",
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
        displayLabel: "current · rev:22222222",
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
        displayLabel: "context · rev:44444444",
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
  it("paints the exact server-sized Revision label", () => {
    const graph = changeGraph();
    const node = graph.nodes[0];
    if (!node) throw new Error("fixture needs a Change Revision node");
    node.displayLabel = "current · revision_v2:33333333";
    const figure = renderChangeRevisionGraph(graph, {
      document,
      onActivateRevision: vi.fn(),
    });
    expect(
      figure.querySelector(`[data-graph-node-id="${node.id}"] text`)
        ?.textContent,
    ).toBe(node.displayLabel);
  });

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
        displayLabel: "observation · observation:44444444",
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
        displayLabel: "observation · observation:55555555",
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
        displayLabel: "assessment · assessment:66666666",
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
        displayLabel: "assessment · assessment:77777777",
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
        displayLabel: "Revision · rev:22222222",
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

function largeFactGraph(nodeCount = 36): FactRelationshipGraphPresentation {
  const nodeWidth = 150;
  const columnWidth = 180;
  const nodes = Array.from({ length: nodeCount }, (_, index) => {
    const serial = (index + 1).toString(16).padStart(64, "0");
    const inverseSerial = (nodeCount - index).toString(16).padStart(64, "0");
    const revision: RevisionRef = {
      revisionId: `rev:sha256:${serial}`,
      objectArtifactContentHash: `sha256:${inverseSerial}`,
    };
    const factId = `observation:sha256:${serial}`;
    return {
      id: factKey(revision, "observation", factId),
      kind: "fact" as const,
      revision,
      factId,
      family: "observation",
      displayLabel: `observation · observation:${serial.slice(0, 8)}`,
      x: nodeWidth / 2 + index * columnWidth,
      y: index % 2 === 0 ? 40 : 110,
      w: nodeWidth,
      h: 34,
      contextAvailability: "available" as const,
      activationRevision: revision,
    };
  });
  return {
    nodes,
    observationSupersedes: [],
    assessmentReplaces: [],
    factPorts: [],
    bounds: {
      w: nodeWidth + (nodeCount - 1) * columnWidth,
      h: 150,
    },
  };
}

function renderLargeFactGraph(
  graph = largeFactGraph(),
): ReturnType<typeof renderFactRelationshipGraph> {
  return renderFactRelationshipGraph(graph, {
    document,
    onActivateRevision: vi.fn(),
    onFocusFact: vi.fn(),
  });
}

describe("exact fact graph renderer", () => {
  it("paints server-sized labels verbatim for otherwise unfamiliar opaque identities", () => {
    const graph = factGraph();
    const fact = graph.nodes.find(
      (node) => node.kind === "fact" && node.factId === newAssessment,
    );
    const revision = graph.nodes.find((node) => node.kind === "revision");
    if (!fact || !revision)
      throw new Error("fixture needs both graph node kinds");
    fact.displayLabel = "assessment · assess_v2:77777777";
    revision.displayLabel = "Revision · revision_v2:22222222";

    const figure = renderLargeFactGraph(graph);
    expect(
      figure.querySelector(`[data-graph-node-id="${fact.id}"] text`)
        ?.textContent,
    ).toBe(fact.displayLabel);
    expect(
      figure.querySelector(`[data-graph-node-id="${revision.id}"] text`)
        ?.textContent,
    ).toBe(revision.displayLabel);
  });

  it.each([
    ["wide", 1_200],
    ["narrow", 360],
  ])("keeps a 36-node canvas wider than its %s viewport", (_layout, viewportWidth) => {
    const graph = largeFactGraph();
    const figure = renderLargeFactGraph(graph);
    const viewport = figure.querySelector<HTMLElement>("[data-graph-viewport]");
    expect(viewport).not.toBeNull();
    if (!viewport) return;

    Object.defineProperty(viewport, "clientWidth", {
      configurable: true,
      value: viewportWidth,
    });
    const canvas = viewport.querySelector<SVGSVGElement>(
      ".fact-relationship-graph-svg",
    );
    const canvasWidth = Number(canvas?.getAttribute("width"));
    expect(canvasWidth).toBe(graph.bounds.w);
    expect(canvasWidth).toBeGreaterThan(viewport.clientWidth);
    expect(APP_CSS).toMatch(
      /\.relationship-graph-viewport\s*\{[^}]*overflow-x:\s*auto;/s,
    );
    expect(APP_CSS).toMatch(
      /\.change-revision-graph-svg,\s*\.fact-relationship-graph-svg\s*\{[^}]*max-width:\s*none;/s,
    );
  });

  it("makes the graph viewport focusable and supports bounded horizontal keys", () => {
    const graph = largeFactGraph();
    const viewport = renderLargeFactGraph(graph).querySelector<HTMLElement>(
      "[data-graph-viewport]",
    );
    expect(viewport).not.toBeNull();
    if (!viewport) return;

    expect(viewport.tabIndex).toBe(0);
    Object.defineProperties(viewport, {
      clientWidth: { configurable: true, value: 600 },
      scrollWidth: { configurable: true, value: graph.bounds.w },
    });

    const arrowRight = new KeyboardEvent("keydown", {
      key: "ArrowRight",
      bubbles: true,
      cancelable: true,
    });
    viewport.dispatchEvent(arrowRight);
    expect(arrowRight.defaultPrevented).toBe(true);
    expect(viewport.scrollLeft).toBeGreaterThan(0);

    const end = new KeyboardEvent("keydown", {
      key: "End",
      bubbles: true,
      cancelable: true,
    });
    viewport.dispatchEvent(end);
    expect(end.defaultPrevented).toBe(true);
    expect(viewport.scrollLeft).toBe(graph.bounds.w - viewport.clientWidth);

    viewport.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "ArrowRight",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(viewport.scrollLeft).toBe(graph.bounds.w - viewport.clientWidth);

    const home = new KeyboardEvent("keydown", {
      key: "Home",
      bubbles: true,
      cancelable: true,
    });
    viewport.dispatchEvent(home);
    expect(home.defaultPrevented).toBe(true);
    expect(viewport.scrollLeft).toBe(0);

    viewport.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "ArrowLeft",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(viewport.scrollLeft).toBe(0);
  });

  it("keeps every full identity in the 36-node textual equivalent", () => {
    const graph = largeFactGraph();
    const equivalent = renderLargeFactGraph(graph).querySelector<HTMLElement>(
      "[data-graph-textual-equivalent]",
    );
    const items = Array.from(
      equivalent?.querySelectorAll<HTMLLIElement>(
        "[data-graph-text-nodes] > li",
      ) ?? [],
    );
    expect(items).toHaveLength(graph.nodes.length);
    for (const node of graph.nodes) {
      if (node.kind !== "fact") continue;
      const exactName = items
        .map(
          (item) =>
            item.querySelector("button")?.getAttribute("aria-label") ??
            item.textContent ??
            "",
        )
        .find((name) => name.includes(node.factId));
      expect(exactName).toContain(node.revision.revisionId);
      expect(exactName).toContain(node.revision.objectArtifactContentHash);
    }
  });

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
    expect(fact?.dataset.revisionId).toBe(first.revisionId);
    expect(fact?.dataset.artifactHash).toBe(first.objectArtifactContentHash);
    expect(fact?.querySelector("text")?.textContent).toBe(
      "observation · observation:55555555",
    );
    expect(fact?.dataset.graphFactFocus).toBe(
      JSON.stringify({
        revisionId: first.revisionId,
        objectArtifactContentHash: first.objectArtifactContentHash,
        family: "observation",
        factId: newObservation,
      }),
    );
    expect(
      figure.querySelector("[data-graph-textual-equivalent]")?.textContent,
    ).toContain(newObservation);
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
