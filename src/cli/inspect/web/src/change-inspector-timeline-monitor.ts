/**
 * Session-only Timeline follow/park policy.
 *
 * The server remains authoritative for the filtered page and its signed
 * continuation. This small controller only decides whether a newly fetched,
 * eligible head page may replace the reader's currently parked window.
 */

import {
  type ChangeInspectorRoute,
  formatChangeInspectorRoute,
} from "./change-inspector-router";
import type { EventHistoryDocument } from "./change-protocol";

export interface TimelineMonitorSnapshot {
  mode: "following" | "parked";
  newCount: number;
  display: EventHistoryDocument;
}

function monitorKey(
  route: Extract<ChangeInspectorRoute, { kind: "timeline" }>,
): string | null {
  const query = route.historyQuery;
  // Only the initial, descending page is a stable head to follow. Historical
  // anchors and opaque continuations are deliberate reading positions and
  // must never jump beneath the reader.
  if (
    query.after !== undefined ||
    query.at !== undefined ||
    query.order === "asc"
  )
    return null;
  return formatChangeInspectorRoute(route);
}

/**
 * Count events inserted ahead of the parked head by identity, not merely by a
 * changing aggregate count. A normal descending head page retains at least
 * one parked ID, making its index the reader-visible number of new events.
 *
 * If an entire bounded page rolled over before the next poll, no parked ID is
 * available to compare. In that exceptional case the server's filtered count
 * delta remains the only bounded recovery signal; an unchanged count falls
 * back to the visible replacement window rather than falsely claiming zero.
 */
export function newEventsAheadOfParkedHead(
  parked: EventHistoryDocument,
  latest: EventHistoryDocument,
): number {
  const parkedIds = new Set(parked.entries.map((entry) => entry.eventId));
  if (parkedIds.size > 0) {
    const retainedIndex = latest.entries.findIndex((entry) =>
      parkedIds.has(entry.eventId),
    );
    if (retainedIndex >= 0) return retainedIndex;
  }
  const countDelta = Math.max(0, latest.matchCount - parked.matchCount);
  if (countDelta > 0) return countDelta;
  return parkedIds.size > 0 ? latest.entries.length : 0;
}

/** Create isolated session state; it never touches localStorage or the URL. */
export function createTimelineMonitor() {
  let key: string | null = null;
  let latest: EventHistoryDocument | null = null;
  let parked: EventHistoryDocument | null = null;
  let following = true;

  const snapshot = (): TimelineMonitorSnapshot | null => {
    if (key === null) return null;
    const display = following ? latest : parked;
    if (display === null) return null;
    return {
      mode: following ? "following" : "parked",
      newCount:
        !following && latest !== null
          ? newEventsAheadOfParkedHead(display, latest)
          : 0,
      display,
    };
  };

  const park = (): TimelineMonitorSnapshot | null => {
    if (latest === null) return null;
    if (following) {
      // Capture exactly what the reader is currently looking at. Subsequent
      // polls can advance authority/projection stamps without repainting it.
      parked = latest;
      following = false;
    }
    return snapshot();
  };

  const follow = (): TimelineMonitorSnapshot | null => {
    if (latest === null) return null;
    parked = null;
    following = true;
    return snapshot();
  };

  return {
    observe(
      route: Extract<ChangeInspectorRoute, { kind: "timeline" }>,
      document: EventHistoryDocument,
    ): TimelineMonitorSnapshot | null {
      const nextKey = monitorKey(route);
      if (nextKey === null) {
        key = null;
        latest = document;
        parked = null;
        following = true;
        return null;
      }
      if (key !== nextKey) {
        key = nextKey;
        parked = null;
        following = true;
      }
      latest = document;
      return snapshot();
    },
    toggle(): TimelineMonitorSnapshot | null {
      if (latest === null) return null;
      if (following) {
        park();
      } else {
        follow();
      }
      return snapshot();
    },
    /** Idempotently retain the reader's current head window. */
    park,
    /** Explicit catch-up resumes the newest successfully loaded head page. */
    follow,
    snapshot,
  };
}
