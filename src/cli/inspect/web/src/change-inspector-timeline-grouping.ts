/**
 * The one projection from a loaded Timeline page's entries to the rows the
 * reader sees. Pure: no DOM, no state, no module-level mutation.
 *
 * Grouping is derived from exactly one `EventHistoryDocument`. A group never
 * spans a continuation boundary, is never merged with a group on an adjacent
 * page, and is re-derived from scratch whenever the render key changes. The
 * server has already filtered the entries it returns; this module never
 * filters, merges, renames, or synthesizes events. A group is a container over
 * unchanged `EventHistoryEntry` values.
 *
 * `visualRows` is the projection the renderer and the keyboard cursor both
 * consume, which is what stops render order and cursor order drifting apart.
 */

import type {
  EventHistoryEntry,
  EventHistoryEventType,
} from "./change-protocol";

export type TimelineGroup = {
  kind: "group";
  eventType: EventHistoryEventType;
  members: EventHistoryEntry[];
};

export type TimelineRow =
  | {
      kind: "event";
      entry: EventHistoryEntry;
      /** Set on a member row spliced in from an expanded group. */
      ofGroup?: TimelineGroup;
    }
  | TimelineGroup;

/** Adjacent same-type runs shorter than this stay flat. */
export const GROUP_MIN_RUN = 3;

/** Walk entries in display order and collapse each adjacent same-type run of at least `minRun`. */
export function groupTimelineEntries(
  entries: readonly EventHistoryEntry[],
  minRun: number,
): readonly TimelineRow[] {
  const rows: TimelineRow[] = [];
  let index = 0;
  while (index < entries.length) {
    const first = entries[index];
    if (first === undefined) break;
    const eventType = first.eventType;
    let end = index + 1;
    while (end < entries.length && entries[end]?.eventType === eventType) {
      end += 1;
    }
    const run = entries.slice(index, end);
    if (run.length >= minRun) {
      rows.push({ kind: "group", eventType, members: run });
    } else {
      for (const entry of run) rows.push({ kind: "event", entry });
    }
    index = end;
  }
  return rows;
}

/**
 * The id a row is addressed by: an event's own id, or a group's first member
 * id. The DOM `data-event-id`, the expansion set, and the keyboard cursor all
 * key on this one value.
 */
export function groupKey(row: TimelineRow): string {
  return row.kind === "group"
    ? (row.members[0]?.eventId ?? "")
    : row.entry.eventId;
}

/** Expand the groups named in `expanded` into their member rows, in place. */
export function visualRows(
  groups: readonly TimelineRow[],
  expanded: ReadonlySet<string>,
): readonly TimelineRow[] {
  const rows: TimelineRow[] = [];
  for (const row of groups) {
    if (row.kind === "group" && expanded.has(groupKey(row))) {
      for (const entry of row.members) {
        rows.push({ kind: "event", entry, ofGroup: row });
      }
    } else {
      rows.push(row);
    }
  }
  return rows;
}

/** The ids the keyboard cursor may walk: exactly the visible sequence. */
export function navigableEventIds(
  rows: readonly TimelineRow[],
): readonly string[] {
  return rows.map(groupKey);
}

/** The key of the group containing `eventId` (including its first member), or null. */
export function owningGroupKey(
  groups: readonly TimelineRow[],
  eventId: string,
): string | null {
  for (const row of groups) {
    if (
      row.kind === "group" &&
      row.members.some((member) => member.eventId === eventId)
    ) {
      return groupKey(row);
    }
  }
  return null;
}

/** The owning group of `eventId` only while that group is collapsed. */
export function collapsedGroupAt(
  groups: readonly TimelineRow[],
  eventId: string | null,
  expanded: ReadonlySet<string>,
): string | null {
  if (eventId === null) return null;
  const owner = owningGroupKey(groups, eventId);
  return owner !== null && !expanded.has(owner) ? owner : null;
}
