/**
 * Pure Change-aware relationship-graph painters.
 *
 * The server supplies both semantics and mmdflux geometry. These renderers only
 * paint that document and wire exact, caller-owned actions; they never infer an
 * edge, select a current peer, fetch, or consult application state.
 */

import type {
  ChangeRevisionGraphPresentation,
  FactRef,
  FactRelationshipGraphPresentation,
  RevisionRef,
} from "./change-protocol";

const SVG_NS = "http://www.w3.org/2000/svg";

export interface ExactFactFocus {
  revision: RevisionRef;
  family: string;
  factId: string;
}

export interface ChangeRevisionGraphOptions {
  document: Document;
  onActivateRevision: (revision: RevisionRef) => void;
}

export interface FactRelationshipGraphOptions
  extends ChangeRevisionGraphOptions {
  onFocusFact: (focus: ExactFactFocus) => void;
}

type SvgAttributes = Record<string, string | number | boolean>;

function svgElement<K extends keyof SVGElementTagNameMap>(
  document: Document,
  name: K,
  attributes: SvgAttributes = {},
): SVGElementTagNameMap[K] {
  const element = document.createElementNS(SVG_NS, name);
  for (const [attribute, value] of Object.entries(attributes)) {
    element.setAttribute(attribute, String(value));
  }
  return element;
}

function words(value: string): string {
  return value.replaceAll("_", " ");
}

function exactRevisionIdentity(revision: RevisionRef): string {
  return `exact Revision ${revision.revisionId}; artifact ${revision.objectArtifactContentHash}`;
}

function exactFactIdentity(focus: ExactFactFocus): string {
  return `${words(focus.family)} ${focus.factId}; ${exactRevisionIdentity(focus.revision)}`;
}

function exactFactData(focus: ExactFactFocus): string {
  return JSON.stringify({
    revisionId: focus.revision.revisionId,
    objectArtifactContentHash: focus.revision.objectArtifactContentHash,
    family: focus.family,
    factId: focus.factId,
  });
}

function setRevisionData(element: Element, revision: RevisionRef): void {
  element.setAttribute("data-revision-id", revision.revisionId);
  element.setAttribute(
    "data-artifact-hash",
    revision.objectArtifactContentHash,
  );
}

function wireAction(
  element: Element,
  action: () => void,
  keyboard: boolean,
): void {
  element.addEventListener("click", action);
  if (!keyboard) return;
  element.addEventListener("keydown", (event) => {
    if (!(event instanceof KeyboardEvent)) return;
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    action();
  });
}

function appendTitle(
  document: Document,
  parent: SVGElement,
  value: string,
): void {
  const title = svgElement(document, "title");
  title.textContent = value;
  parent.append(title);
}

function marker(
  document: Document,
  id: string,
  kind: "solid" | "open",
): SVGMarkerElement {
  const result = svgElement(document, "marker", {
    id,
    markerWidth: 9,
    markerHeight: 9,
    refX: 8,
    refY: 4.5,
    orient: "auto",
    markerUnits: "userSpaceOnUse",
  });
  const path = svgElement(document, "path", {
    d: "M0,0 L8,4.5 L0,9 z",
    fill: kind === "solid" ? "currentColor" : "none",
    stroke: "currentColor",
    "stroke-width": kind === "solid" ? 0 : 1.5,
  });
  result.append(path);
  return result;
}

function graphRoot(
  document: Document,
  className: string,
  label: string,
  bounds: { w: number; h: number },
): SVGSVGElement {
  return svgElement(document, "svg", {
    class: className,
    width: bounds.w,
    height: bounds.h,
    viewBox: `0 0 ${bounds.w} ${bounds.h}`,
    preserveAspectRatio: "xMinYMin meet",
    role: "group",
    "aria-label": label,
  });
}

function graphViewport(document: Document, label: string): HTMLDivElement {
  const viewport = document.createElement("div");
  viewport.className = "relationship-graph-viewport";
  viewport.dataset.graphViewport = "true";
  viewport.tabIndex = 0;
  viewport.setAttribute("role", "region");
  viewport.setAttribute(
    "aria-label",
    `${label}; use Left and Right Arrow keys, Home, and End to pan`,
  );
  viewport.addEventListener("keydown", (event) => {
    if (event.target !== viewport) return;
    const maximum = Math.max(0, viewport.scrollWidth - viewport.clientWidth);
    const step = Math.max(80, Math.round(viewport.clientWidth * 0.8));
    let next: number;
    switch (event.key) {
      case "ArrowLeft":
        next = viewport.scrollLeft - step;
        break;
      case "ArrowRight":
        next = viewport.scrollLeft + step;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = maximum;
        break;
      default:
        return;
    }
    event.preventDefault();
    viewport.scrollLeft = Math.min(maximum, Math.max(0, next));
  });
  return viewport;
}

function sorted<T>(values: readonly T[], key: (value: T) => string): T[] {
  return [...values].sort((left, right) =>
    key(left).localeCompare(key(right), "en"),
  );
}

function edgePoints(
  path: Array<[number, number]>,
  from: { x: number; y: number } | undefined,
): string {
  let points = path;
  if (from && path.length > 1) {
    const distance = ([x, y]: [number, number]): number =>
      (x - from.x) ** 2 + (y - from.y) ** 2;
    if (distance(path[0]) < distance(path[path.length - 1])) {
      points = [...path].reverse();
    }
  }
  return points.map(([x, y]) => `${x},${y}`).join(" ");
}

function relationshipGroup(
  document: Document,
  accessibleName: string,
  attributes: SvgAttributes,
  path: Array<[number, number]>,
  from: { x: number; y: number } | undefined,
  markerId: string,
  strokeDasharray?: string,
): SVGGElement {
  const group = svgElement(document, "g", {
    role: "group",
    "aria-label": accessibleName,
    ...attributes,
  });
  appendTitle(document, group, accessibleName);
  group.append(
    svgElement(document, "polyline", {
      points: edgePoints(path, from),
      fill: "none",
      stroke: "currentColor",
      "stroke-width": 2,
      "stroke-dasharray": strokeDasharray ?? "none",
      "vector-effect": "non-scaling-stroke",
      "marker-end": `url(#${markerId})`,
      "aria-hidden": true,
    }),
  );
  return group;
}

function textualEquivalent(
  document: Document,
  label: string,
): {
  root: HTMLDetailsElement;
  nodes: HTMLUListElement;
  edges: HTMLUListElement;
} {
  const root = document.createElement("details");
  root.className = "relationship-graph-text";
  root.dataset.graphTextualEquivalent = "true";
  const summary = document.createElement("summary");
  summary.textContent = `${label} as text`;
  const nodeHeading = document.createElement("h4");
  nodeHeading.textContent = "Exact identities";
  const nodes = document.createElement("ul");
  nodes.dataset.graphTextNodes = "true";
  const edgeHeading = document.createElement("h4");
  edgeHeading.textContent = "Relationships";
  const edges = document.createElement("ul");
  edges.dataset.graphTextEdges = "true";
  root.append(summary, nodeHeading, nodes, edgeHeading, edges);
  return { root, nodes, edges };
}

function actionItem(
  document: Document,
  accessibleName: string,
  action: () => void,
): HTMLLIElement {
  const item = document.createElement("li");
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = accessibleName;
  button.title = accessibleName;
  button.setAttribute("aria-label", accessibleName);
  wireAction(button, action, false);
  item.append(button);
  return item;
}

function textItem(document: Document, value: string): HTMLLIElement {
  const item = document.createElement("li");
  item.textContent = value;
  return item;
}

/** Paint one authoritative Change Revision graph and its textual equivalent. */
export function renderChangeRevisionGraph(
  graph: ChangeRevisionGraphPresentation,
  options: ChangeRevisionGraphOptions,
): HTMLElement {
  const { document } = options;
  const figure = document.createElement("figure");
  figure.className = "change-revision-graph";
  figure.setAttribute("aria-label", "Change Revision relationships");
  const svg = graphRoot(
    document,
    "change-revision-graph-svg",
    "Change Revision relationship graph",
    graph.bounds,
  );
  const viewport = graphViewport(
    document,
    "Change Revision relationship graph",
  );
  viewport.append(svg);
  const defs = svgElement(document, "defs");
  defs.append(
    marker(document, "change-effective-arrow", "solid"),
    marker(document, "change-claim-arrow", "open"),
  );
  svg.append(defs);

  const text = textualEquivalent(document, "Change Revision relationships");
  const nodesById = new Map(graph.nodes.map((node) => [node.id, node]));

  for (const edge of sorted(
    graph.effectiveSupersedes,
    (candidate) => `${candidate.from}\u0000${candidate.to}`,
  )) {
    const accessibleName = `Effective supersedes relationship: ${exactRevisionIdentity(edge.successor)} supersedes ${exactRevisionIdentity(edge.predecessor)}`;
    const group = relationshipGroup(
      document,
      accessibleName,
      {
        class: "change-revision-edge change-revision-edge-effective",
        "data-edge-kind": "effective-supersedes",
        "data-from": edge.from,
        "data-to": edge.to,
      },
      edge.path,
      nodesById.get(edge.from),
      "change-effective-arrow",
    );
    setRevisionData(group, edge.successor);
    group.setAttribute(
      "data-predecessor-revision-id",
      edge.predecessor.revisionId,
    );
    group.setAttribute(
      "data-predecessor-artifact-hash",
      edge.predecessor.objectArtifactContentHash,
    );
    svg.append(group);
    text.edges.append(textItem(document, accessibleName));
  }

  for (const edge of sorted(
    graph.pendingOrConflictingClaims,
    (candidate) =>
      `${candidate.claimId}\u0000${candidate.from}\u0000${candidate.to}`,
  )) {
    const diagnostics = (edge.diagnostics ?? []).length
      ? ` Diagnostics: ${edge.diagnostics?.join("; ")}`
      : "";
    const accessibleName = `Pending or conflicting supersedes claim ${edge.claimId}: ${exactRevisionIdentity(edge.successor)} claims to supersede ${exactRevisionIdentity(edge.predecessor)}.${diagnostics}`;
    const group = relationshipGroup(
      document,
      accessibleName,
      {
        class: "change-revision-edge change-revision-edge-claim",
        "data-edge-kind": "pending-or-conflicting-claim",
        "data-claim-id": edge.claimId,
        "data-from": edge.from,
        "data-to": edge.to,
      },
      edge.path,
      nodesById.get(edge.from),
      "change-claim-arrow",
      "7 5",
    );
    setRevisionData(group, edge.successor);
    group.setAttribute(
      "data-predecessor-revision-id",
      edge.predecessor.revisionId,
    );
    group.setAttribute(
      "data-predecessor-artifact-hash",
      edge.predecessor.objectArtifactContentHash,
    );
    svg.append(group);
    text.edges.append(textItem(document, accessibleName));
  }

  for (const node of sorted(graph.nodes, (candidate) => candidate.id)) {
    const activationRevision = node.activationRevision;
    const canActivate =
      node.contextAvailability === "available" &&
      activationRevision !== undefined;
    const state = [
      node.isCurrent ? "current" : "not current",
      node.isMember ? "Change member" : "claim-only context",
      canActivate
        ? "exact Change context available"
        : "relationship context only; no exact Change route is available",
    ].join("; ");
    const accessibleName = `${exactRevisionIdentity(node.revision)}; ${state}`;
    const group = svgElement(document, "g", {
      class: `change-revision-node${node.isCurrent ? " is-current" : ""}${node.isMember ? " is-member" : " is-context"}`,
      role: canActivate ? "link" : "group",
      "aria-label": accessibleName,
      "data-graph-node-id": node.id,
      "data-current": node.isCurrent,
      "data-member": node.isMember,
      "data-context-availability": node.contextAvailability,
    });
    if (canActivate) {
      group.setAttribute("tabindex", "0");
    } else {
      group.setAttribute("aria-disabled", "true");
    }
    setRevisionData(group, node.revision);
    group.setAttribute("title", accessibleName);
    appendTitle(document, group, accessibleName);
    group.append(
      svgElement(document, "rect", {
        x: node.x - node.w / 2,
        y: node.y - node.h / 2,
        width: node.w,
        height: node.h,
        rx: 6,
        fill: "none",
        stroke: "currentColor",
        "stroke-width": node.isCurrent ? 3 : node.isMember ? 2 : 1,
        "stroke-dasharray": node.isMember ? "none" : "4 3",
        "vector-effect": "non-scaling-stroke",
        "aria-hidden": true,
      }),
    );
    const label = svgElement(document, "text", {
      x: node.x,
      y: node.y,
      "text-anchor": "middle",
      "dominant-baseline": "middle",
      "aria-hidden": true,
    });
    label.textContent = node.displayLabel;
    group.append(label);
    const activate = canActivate
      ? (): void => options.onActivateRevision(activationRevision)
      : undefined;
    if (activate) wireAction(group, activate, true);
    svg.append(group);
    text.nodes.append(
      activate
        ? actionItem(document, `Open ${accessibleName}`, activate)
        : textItem(document, accessibleName),
    );
  }

  for (const diagnostic of graph.diagnostics ?? []) {
    text.edges.append(textItem(document, `Graph diagnostic: ${diagnostic}`));
  }
  figure.append(viewport, text.root);
  return figure;
}

function factRefIdentity(reference: FactRef): {
  family: string;
  factId: string;
} {
  if (reference.kind === "observation") {
    return { family: reference.kind, factId: reference.observationId ?? "" };
  }
  return { family: reference.kind, factId: reference.inputRequestId ?? "" };
}

function appendFactEdge(
  document: Document,
  svg: SVGSVGElement,
  text: HTMLUListElement,
  nodesById: Map<string, { x: number; y: number }>,
  edge: { from: string; to: string; path: Array<[number, number]> },
  accessibleName: string,
  kind: string,
  markerId: string,
  dash?: string,
): SVGGElement {
  const group = relationshipGroup(
    document,
    accessibleName,
    {
      class: `fact-relationship-edge fact-relationship-edge-${kind}`,
      "data-edge-kind": kind,
      "data-from": edge.from,
      "data-to": edge.to,
    },
    edge.path,
    nodesById.get(edge.from),
    markerId,
    dash,
  );
  svg.append(group);
  text.append(textItem(document, accessibleName));
  return group;
}

/** Paint one exact contextual fact graph and its textual equivalent. */
export function renderFactRelationshipGraph(
  graph: FactRelationshipGraphPresentation,
  options: FactRelationshipGraphOptions,
): HTMLElement {
  const { document } = options;
  const figure = document.createElement("figure");
  figure.className = "fact-relationship-graph";
  figure.setAttribute("aria-label", "Exact fact relationships");
  const svg = graphRoot(
    document,
    "fact-relationship-graph-svg",
    "Exact fact relationship graph",
    graph.bounds,
  );
  const viewport = graphViewport(document, "Exact fact relationship graph");
  viewport.append(svg);
  const defs = svgElement(document, "defs");
  defs.append(
    marker(document, "fact-observation-arrow", "solid"),
    marker(document, "fact-assessment-arrow", "open"),
    marker(document, "fact-port-arrow", "open"),
  );
  svg.append(defs);
  const text = textualEquivalent(document, "Exact fact relationships");
  const nodesById = new Map(graph.nodes.map((node) => [node.id, node]));

  for (const edge of sorted(
    graph.observationSupersedes,
    (candidate) => `${candidate.from}\u0000${candidate.to}`,
  )) {
    const accessibleName = `Observation supersedes relationship: observation ${edge.fromFactId}; ${exactRevisionIdentity(edge.originRevision)} supersedes observation ${edge.toFactId}; ${exactRevisionIdentity(edge.originRevision)}`;
    const group = appendFactEdge(
      document,
      svg,
      text.edges,
      nodesById,
      edge,
      accessibleName,
      "observation-supersedes",
      "fact-observation-arrow",
    );
    setRevisionData(group, edge.originRevision);
    group.setAttribute("data-graph-from-fact-id", edge.fromFactId);
    group.setAttribute("data-graph-to-fact-id", edge.toFactId);
  }

  for (const edge of sorted(
    graph.assessmentReplaces,
    (candidate) => `${candidate.from}\u0000${candidate.to}`,
  )) {
    const accessibleName = `Assessment replaces relationship: assessment ${edge.fromFactId}; ${exactRevisionIdentity(edge.originRevision)} replaces assessment ${edge.toFactId}; ${exactRevisionIdentity(edge.originRevision)}`;
    const group = appendFactEdge(
      document,
      svg,
      text.edges,
      nodesById,
      edge,
      accessibleName,
      "assessment-replaces",
      "fact-assessment-arrow",
      "10 4",
    );
    setRevisionData(group, edge.originRevision);
    group.setAttribute("data-graph-from-fact-id", edge.fromFactId);
    group.setAttribute("data-graph-to-fact-id", edge.toFactId);
  }

  for (const edge of sorted(
    graph.factPorts,
    (candidate) =>
      `${candidate.portId}\u0000${candidate.from}\u0000${candidate.to}`,
  )) {
    const origin = factRefIdentity(edge.originFact);
    const originIdentity = exactFactIdentity({
      revision: edge.originRevision,
      ...origin,
    });
    const target = edge.targetFact
      ? exactFactIdentity({
          revision: edge.targetRevision,
          ...factRefIdentity(edge.targetFact),
        })
      : exactRevisionIdentity(edge.targetRevision);
    const diagnostics = (edge.diagnostics ?? []).length
      ? ` Diagnostics: ${edge.diagnostics?.join("; ")}`
      : "";
    const accessibleName = `Fact port ${edge.portId}: ${originIdentity} ${words(edge.relation)} ${target}; applicability ${words(edge.applicability)}.${diagnostics}`;
    const group = appendFactEdge(
      document,
      svg,
      text.edges,
      nodesById,
      edge,
      accessibleName,
      "fact-port",
      "fact-port-arrow",
      "2 4",
    );
    group.setAttribute("data-port-id", edge.portId);
    group.setAttribute("data-port-relation", edge.relation);
    group.setAttribute("data-port-applicability", edge.applicability);
    setRevisionData(group, edge.originRevision);
    group.setAttribute(
      "data-target-revision-id",
      edge.targetRevision.revisionId,
    );
    group.setAttribute(
      "data-target-artifact-hash",
      edge.targetRevision.objectArtifactContentHash,
    );
  }

  for (const node of sorted(graph.nodes, (candidate) => candidate.id)) {
    const isFact = node.kind === "fact";
    const focus =
      isFact && node.factId !== undefined && node.family !== undefined
        ? {
            revision: node.revision,
            family: node.family,
            factId: node.factId,
          }
        : undefined;
    const activationRevision = node.activationRevision;
    const canActivate =
      node.contextAvailability === "available" &&
      activationRevision !== undefined;
    const availability = canActivate
      ? `exact Change context available in ${exactRevisionIdentity(activationRevision)}`
      : "relationship context only; no exact Change route is available";
    const accessibleName = focus
      ? `${canActivate ? "Focus" : "Relationship context for"} exact fact ${exactFactIdentity(focus)}; ${availability}`
      : `${canActivate ? "Open" : "Relationship context for"} ${exactRevisionIdentity(node.revision)} fact-port anchor; ${availability}`;
    const group = svgElement(document, "g", {
      class: `fact-relationship-node fact-relationship-node-${isFact ? "fact" : "revision"}`,
      role: canActivate ? "link" : "group",
      "aria-label": accessibleName,
      "data-graph-node-id": node.id,
      "data-node-kind": node.kind,
      "data-context-availability": node.contextAvailability,
    });
    if (canActivate) {
      group.setAttribute("tabindex", "0");
    } else {
      group.setAttribute("aria-disabled", "true");
    }
    setRevisionData(group, node.revision);
    group.setAttribute("title", accessibleName);
    if (focus) {
      // These identities intentionally remain graph-specific. Exact route focus
      // resolves authored fact cards through `data-fact-id`; reusing that
      // attribute here would focus the SVG node before the readable card.
      group.setAttribute("data-graph-family", focus.family);
      group.setAttribute("data-graph-fact-id", focus.factId);
      group.setAttribute("data-graph-fact-focus", exactFactData(focus));
    }
    appendTitle(document, group, accessibleName);
    group.append(
      svgElement(document, "rect", {
        x: node.x - node.w / 2,
        y: node.y - node.h / 2,
        width: node.w,
        height: node.h,
        rx: isFact ? 6 : 0,
        fill: "none",
        stroke: "currentColor",
        "stroke-width": 2,
        "stroke-dasharray": isFact ? "none" : "4 3",
        "vector-effect": "non-scaling-stroke",
        "aria-hidden": true,
      }),
    );
    const label = svgElement(document, "text", {
      x: node.x,
      y: node.y,
      "text-anchor": "middle",
      "dominant-baseline": "middle",
      "aria-hidden": true,
    });
    label.textContent = node.displayLabel;
    group.append(label);
    const activate = canActivate
      ? focus
        ? (): void =>
            options.onFocusFact({
              ...focus,
              revision: activationRevision,
            })
        : (): void => options.onActivateRevision(activationRevision)
      : undefined;
    if (activate) wireAction(group, activate, true);
    svg.append(group);
    text.nodes.append(
      activate
        ? actionItem(document, accessibleName, activate)
        : textItem(document, accessibleName),
    );
  }

  figure.append(viewport, text.root);
  return figure;
}
