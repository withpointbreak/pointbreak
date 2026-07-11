// The generic, node-id-agnostic supersession-DAG painter for the composite
// page's supersession graphs. Pure: geometry in, SVG string out. The SVG root
// keeps the `revision-dag` class so descendants inherit the shared DAG
// stylesheet (the `--dag-edge` var, node/edge rules). What varies per caller is
// node identity semantics — the id attribute, whether nodes are interactive,
// the aria noun, and the selection predicate.
import { CLASS, dagNodeClass } from "./classNames";
import { escapeHtml } from "./escape";
import type { ThreadLayout } from "./model";
import { shortId } from "./refs";

/** How a supersession graph's nodes are wired for a given lens. */
export interface SupersessionSvgOptions {
  /** The dataset attribute each node carries, e.g. `data-revision-id` / `data-fact-id`. */
  idAttr: string;
  /** The per-node aria-label noun (`revision` / `assessment` / `observation`). */
  ariaNoun: string;
  /** When true, nodes are keyboard-focusable links (`tabindex="0" role="link"`). */
  interactive: boolean;
  /** Whether a node id is the current selection (drives `aria-selected`). */
  isSelected: (id: string) => boolean;
}

/** Render a laid-out supersession DAG as an SVG string, or "" when there is no layout. */
export function renderSupersessionSvg(
  laid: ThreadLayout | null | undefined,
  opts: SupersessionSvgOptions,
): string {
  const nodes = laid?.nodes ?? [];
  if (!laid || !nodes.length) return "";
  const w = laid.bounds?.w ?? 0;
  const h = laid.bounds?.h ?? 0;
  // Node centres, so each edge's arrowhead is oriented by node identity: the arrow
  // points at the superseding `from` head, never by raw points order (a
  // reversed/cycle edge still renders correctly).
  const center = new Map<string, [number, number]>(
    nodes.map((n): [string, [number, number]] => [
      n.id ?? "",
      [n.x ?? 0, n.y ?? 0],
    ]),
  );
  // Two shared arrowhead markers: a default (border-coloured) and a traced
  // (accent-coloured) one. The DAG wiring swaps a traced edge to the accent marker
  // — a cross-browser alternative to `context-stroke` (which Safari does not paint).
  const marker = (id: string, cls: string): string =>
    `<marker id="${id}" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto" markerUnits="userSpaceOnUse"><path class="${cls}" d="M0,0 L7,4 L0,8 z" /></marker>`;
  const defs = `<defs>${marker("dag-arrow", CLASS.dagArrowHead)}${marker("dag-arrow-traced", CLASS.dagArrowHeadTraced)}</defs>`;
  const edges = (laid.edges ?? [])
    .map((e) => {
      // Draw so the LAST point is nearest the `from` (superseding head) node;
      // marker-end then points the arrowhead at that head, so succession reads
      // bottom-up (a fork diverges upward into its competing heads).
      let path = e.path ?? [];
      const from = e.from != null ? center.get(e.from) : undefined;
      if (from && path.length > 1) {
        const dist2 = (p: number[]): number =>
          (p[0] - from[0]) ** 2 + (p[1] - from[1]) ** 2;
        if (dist2(path[0]) < dist2(path[path.length - 1]))
          path = [...path].reverse();
      }
      const pts = path.map(([x, y]) => `${x},${y}`).join(" ");
      return `<polyline class="${CLASS.dagEdge}" data-from="${escapeHtml(e.from ?? "")}" data-to="${escapeHtml(e.to ?? "")}" points="${pts}" marker-end="url(#dag-arrow)" />`;
    })
    .join("");
  const nodesHtml = nodes
    .map((n) => {
      const id = n.id ?? "";
      const sel = opts.isSelected(id);
      const nodeW = n.w ?? 0;
      const nodeH = n.h ?? 0;
      const nx = n.x ?? 0;
      const ny = n.y ?? 0;
      const cls = dagNodeClass({
        isHead: !!n.isHead,
        isSuperseded: !!n.isSuperseded,
      });
      const interactive = opts.interactive ? ' tabindex="0" role="link"' : "";
      const selected = sel ? ' aria-selected="true"' : "";
      return `<g class="${cls}" ${opts.idAttr}="${escapeHtml(id)}"${interactive}${selected} aria-label="${escapeHtml(opts.ariaNoun)} ${escapeHtml(shortId(id))}">
        <rect x="${nx - nodeW / 2}" y="${ny - nodeH / 2}" width="${nodeW}" height="${nodeH}" rx="6" />
        <text x="${nx}" y="${ny}" text-anchor="middle" dominant-baseline="middle">${escapeHtml(shortId(id))}</text>
      </g>`;
    })
    .join("");
  // Render at natural pixel size (1 user unit = 1px) so the node text is not scaled
  // to illegibility; CSS `max-width:100%` shrinks an oversized graph proportionally.
  return `<svg class="${CLASS.revisionDag}" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" preserveAspectRatio="xMinYMin meet" role="group" aria-label="supersession graph">${defs}${edges}${nodesHtml}</svg>`;
}
