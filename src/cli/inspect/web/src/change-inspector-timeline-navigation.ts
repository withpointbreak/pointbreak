/**
 * Pure cursor policy for the bounded Timeline window.
 *
 * The server owns chronology and opaque continuations. This module therefore
 * never guesses at an event outside the rendered page: a boundary move asks
 * its DOM owner to follow the adjacent signed continuation and describes where
 * the selection should land once that page arrives.
 */

export interface TimelineWindow {
  eventIds: readonly string[];
  selectedEventId: string | null;
}

export type TimelineSelectionIntent =
  | { kind: "select"; eventId: string }
  | {
      kind: "adjacent-page";
      direction: "previous" | "next";
      /** Zero-based index from the start of the next page. */
      index?: number;
      /** Zero-based index from the end of the previous page. */
      indexFromEnd?: number;
    };

function selectedIndex(window: TimelineWindow): number {
  return window.selectedEventId === null
    ? -1
    : window.eventIds.indexOf(window.selectedEventId);
}

/** Move by a number of rows, crossing at most one adjacent server page. */
export function moveTimelineSelection(
  window: TimelineWindow,
  delta: number,
): TimelineSelectionIntent | null {
  const { eventIds } = window;
  if (eventIds.length === 0 || delta === 0) return null;
  const current = selectedIndex(window);
  // With no local selection, both directions begin at the first readable row.
  // This mirrors the old Timeline cursor: the first j/k establishes a cursor;
  // it does not silently skip an event.
  if (current < 0) {
    const eventId = eventIds[0];
    return eventId ? { kind: "select", eventId } : null;
  }
  const start = current;
  const target = start + delta;
  if (target >= 0 && target < eventIds.length) {
    const eventId = eventIds[target];
    return eventId ? { kind: "select", eventId } : null;
  }
  if (target >= eventIds.length) {
    return {
      kind: "adjacent-page",
      direction: "next",
      index: target - eventIds.length,
    };
  }
  return {
    kind: "adjacent-page",
    direction: "previous",
    indexFromEnd: Math.abs(target) - 1,
  };
}

/** Select the nearest visible boundary without inventing a global offset. */
export function boundaryTimelineSelection(
  window: TimelineWindow,
  boundary: "first" | "last",
): TimelineSelectionIntent | null {
  const eventId =
    boundary === "first" ? window.eventIds[0] : window.eventIds.at(-1);
  return eventId ? { kind: "select", eventId } : null;
}

/**
 * Translate a full/half viewport command into the same bounded movement
 * contract. The caller supplies the measured visible row count and can retain
 * its selection across a continuation request using the resulting intent.
 */
export function pageTimelineSelection(
  window: TimelineWindow,
  direction: "backward" | "forward",
  visibleRows: number,
  fraction: "full" | "half",
): TimelineSelectionIntent | null {
  const rows = Math.max(1, Math.floor(visibleRows));
  const delta = fraction === "full" ? rows : Math.max(1, Math.ceil(rows / 2));
  return moveTimelineSelection(
    window,
    direction === "forward" ? delta : -delta,
  );
}

/** Resolve an adjacent-page landing anchor against the newly rendered window. */
export function resolveTimelinePageSelection(
  eventIds: readonly string[],
  intent: Extract<TimelineSelectionIntent, { kind: "adjacent-page" }>,
): string | null {
  if (eventIds.length === 0) return null;
  if (intent.direction === "next") {
    return eventIds[Math.min(intent.index ?? 0, eventIds.length - 1)] ?? null;
  }
  return (
    eventIds[Math.max(0, eventIds.length - 1 - (intent.indexFromEnd ?? 0))] ??
    null
  );
}
