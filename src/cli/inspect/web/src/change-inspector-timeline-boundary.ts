/**
 * Honest global Timeline boundary traversal.
 *
 * A continuation is an opaque server capability, not an offset the browser may
 * decode or synthesize.  Finding the tail therefore starts from the filtered
 * first page and follows only `next` values issued by that same immutable
 * generation.  This module owns no DOM or route history; it returns the exact
 * bounded route the composition may navigate to after validation succeeds.
 */

import type { ChangeInspectorRoute } from "./change-inspector-router";
import { ChangeInspectorGenerationChanged } from "./change-inspector-state";
import type {
  EventHistoryDocument,
  EventHistoryQuery,
} from "./change-protocol";

type TimelineRoute = Extract<ChangeInspectorRoute, { kind: "timeline" }>;

export class ChangeInspectorTimelineTraversalRefused extends Error {}

function canonical(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>).sort(
      ([left], [right]) => left.localeCompare(right),
    );
    return `{${entries
      .map(([key, nested]) => `${JSON.stringify(key)}:${canonical(nested)}`)
      .join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
}

/** Strip historical positioning while preserving every filtering/order field. */
export function firstTimelineRoute(route: TimelineRoute): TimelineRoute {
  return {
    kind: "timeline",
    historyQuery: {
      ...route.historyQuery,
      after: undefined,
      at: undefined,
    },
  };
}

function requireSameTimelineGeneration(
  anchor: EventHistoryDocument,
  page: EventHistoryDocument,
): void {
  if (
    page.sourceChangeProjectionStamp !== anchor.sourceChangeProjectionStamp ||
    page.timelineProjectionStamp !== anchor.timelineProjectionStamp ||
    page.eventCount !== anchor.eventCount ||
    page.matchCount !== anchor.matchCount ||
    page.order !== anchor.order ||
    canonical(page.authorityCursor) !== canonical(anchor.authorityCursor)
  ) {
    throw new ChangeInspectorGenerationChanged();
  }
}

export interface TimelineTailTraversal {
  route: TimelineRoute;
  page: EventHistoryDocument;
  pageCount: number;
}

/**
 * Follow a filtered generation to its real tail without interpreting a token.
 * `matchCount` and the requested limit bound work; offset continuity and a
 * token set refuse malformed or cyclic server chains.
 */
export async function traverseTimelineTail(
  route: TimelineRoute,
  anchor: EventHistoryDocument,
  load: (query: EventHistoryQuery) => Promise<EventHistoryDocument>,
): Promise<TimelineTailTraversal> {
  const first = firstTimelineRoute(route);
  const limit = first.historyQuery.limit ?? 100;
  const maximumPages = Math.max(1, Math.ceil(anchor.matchCount / limit));
  let page = await load(first.historyQuery);
  requireSameTimelineGeneration(anchor, page);
  if (page.offset !== 0) {
    throw new ChangeInspectorTimelineTraversalRefused(
      "Timeline first page did not begin at the filtered head",
    );
  }

  let pageCount = 1;
  let finalAfter: string | undefined;
  const seen = new Set<string>();
  while (page.next !== undefined) {
    const token = page.next;
    if (seen.has(token)) {
      throw new ChangeInspectorTimelineTraversalRefused(
        "Timeline continuation chain contained a cycle",
      );
    }
    if (pageCount >= maximumPages || page.entries.length === 0) {
      throw new ChangeInspectorTimelineTraversalRefused(
        "Timeline continuation chain exceeded its bounded match count",
      );
    }
    seen.add(token);
    const expectedOffset = page.offset + page.entries.length;
    const next = await load({ ...first.historyQuery, after: token });
    requireSameTimelineGeneration(anchor, next);
    if (next.offset !== expectedOffset) {
      throw new ChangeInspectorTimelineTraversalRefused(
        "Timeline continuation chain was not contiguous",
      );
    }
    page = next;
    finalAfter = token;
    pageCount += 1;
  }

  return {
    route: {
      kind: "timeline",
      historyQuery: { ...first.historyQuery, after: finalAfter },
    },
    page,
    pageCount,
  };
}
