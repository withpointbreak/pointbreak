import { describe, expect, it } from "vitest";
import {
  collapsedGroupAt,
  GROUP_MIN_RUN,
  groupKey,
  groupTimelineEntries,
  navigableEventIds,
  owningGroupKey,
  summarizeTimelineGroup,
  type TimelineGroup,
  visualRows,
} from "../src/change-inspector-timeline-grouping";
import type { EventHistoryEntry } from "../src/change-protocol";

const entry = (eventId: string, eventType: string): EventHistoryEntry =>
  ({ eventId, eventType }) as unknown as EventHistoryEntry;

const mixedPage = () => [
  entry("ev:a1", "review_assessment_recorded"),
  entry("ev:b2", "validation_check_recorded"),
  entry("ev:c3", "validation_check_recorded"),
  entry("ev:d4", "validation_check_recorded"),
  entry("ev:e5", "review_observation_recorded"),
];

describe("groupTimelineEntries", () => {
  it("collapses a run at or above the threshold and leaves shorter runs flat", () => {
    const rows = groupTimelineEntries(mixedPage(), 3);

    expect(rows.map((row) => row.kind)).toEqual(["event", "group", "event"]);
    expect(rows[1]).toMatchObject({
      kind: "group",
      eventType: "validation_check_recorded",
    });
    expect(
      rows[1]?.kind === "group" ? rows[1].members.map((m) => m.eventId) : [],
    ).toEqual(["ev:b2", "ev:c3", "ev:d4"]);
  });

  it("leaves a run below the threshold ungrouped", () => {
    const rows = groupTimelineEntries(
      [
        entry("ev:g7", "validation_check_recorded"),
        entry("ev:h8", "validation_check_recorded"),
      ],
      3,
    );

    expect(rows.map((row) => row.kind)).toEqual(["event", "event"]);
  });

  it("returns every entry flat when no run exists", () => {
    const rows = groupTimelineEntries(
      [
        entry("ev:a1", "review_assessment_recorded"),
        entry("ev:b2", "validation_check_recorded"),
        entry("ev:c3", "review_observation_recorded"),
      ],
      3,
    );

    expect(rows.map((row) => row.kind)).toEqual(["event", "event", "event"]);
  });

  it("collapses a run at the head of the page", () => {
    const rows = groupTimelineEntries(
      [
        entry("ev:a1", "validation_check_recorded"),
        entry("ev:b2", "validation_check_recorded"),
        entry("ev:c3", "validation_check_recorded"),
        entry("ev:d4", "review_observation_recorded"),
      ],
      3,
    );

    expect(rows.map((row) => row.kind)).toEqual(["group", "event"]);
    expect(groupKey(rows[0] as never)).toBe("ev:a1");
  });

  it("collapses a run at the tail of the page", () => {
    const rows = groupTimelineEntries(
      [
        entry("ev:a1", "review_observation_recorded"),
        entry("ev:b2", "validation_check_recorded"),
        entry("ev:c3", "validation_check_recorded"),
        entry("ev:d4", "validation_check_recorded"),
      ],
      3,
    );

    expect(rows.map((row) => row.kind)).toEqual(["event", "group"]);
    expect(
      rows[1]?.kind === "group" ? rows[1].members.map((m) => m.eventId) : [],
    ).toEqual(["ev:b2", "ev:c3", "ev:d4"]);
  });

  it("never reads beyond one document: a run split by a page boundary stays flat", () => {
    // Two adjacent pages each end/start with two validations. Across the
    // boundary the run would be four long, but each page is grouped alone.
    const pageOne = [
      entry("ev:a1", "review_observation_recorded"),
      entry("ev:b2", "validation_check_recorded"),
      entry("ev:c3", "validation_check_recorded"),
    ];
    const pageTwo = [
      entry("ev:d4", "validation_check_recorded"),
      entry("ev:e5", "validation_check_recorded"),
      entry("ev:f6", "review_observation_recorded"),
    ];

    expect(groupTimelineEntries(pageOne, 3).map((row) => row.kind)).toEqual([
      "event",
      "event",
      "event",
    ]);
    expect(groupTimelineEntries(pageTwo, 3).map((row) => row.kind)).toEqual([
      "event",
      "event",
      "event",
    ]);
    // The result is a function of the one array passed in.
    const frozen = Object.freeze([...pageOne]);
    expect(groupTimelineEntries(frozen, 3)).toHaveLength(3);
  });

  it("is total over an empty page", () => {
    expect(groupTimelineEntries([], 3)).toEqual([]);
    expect(navigableEventIds(visualRows([], new Set()))).toEqual([]);
  });

  it("groups only adjacent runs, so a separated same-type pair stays flat", () => {
    const rows = groupTimelineEntries(
      [
        entry("ev:a1", "validation_check_recorded"),
        entry("ev:b2", "review_observation_recorded"),
        entry("ev:c3", "validation_check_recorded"),
        entry("ev:d4", "validation_check_recorded"),
        entry("ev:e5", "validation_check_recorded"),
        entry("ev:f6", "validation_check_recorded"),
      ],
      3,
    );

    expect(rows.map((row) => row.kind)).toEqual(["event", "event", "group"]);
    expect(rows[2]?.kind === "group" ? rows[2].members.length : 0).toBe(4);
  });

  it("preserves chronology: group position is its first member's position", () => {
    const source = [
      entry("ev:b2", "validation_check_recorded"),
      entry("ev:c3", "validation_check_recorded"),
      entry("ev:d4", "validation_check_recorded"),
      entry("ev:e5", "review_observation_recorded"),
    ];
    const rows = groupTimelineEntries(source, 3);
    const expanded = new Set(["ev:b2"]);

    expect(navigableEventIds(visualRows(rows, expanded))).toEqual(
      source.map((item) => item.eventId),
    );
  });

  it("exports the one collapse threshold the design fixes", () => {
    expect(GROUP_MIN_RUN).toBe(3);
  });
});

describe("visualRows", () => {
  it("keys expansion by the group's first member event id", () => {
    const rows = groupTimelineEntries(
      [
        entry("ev:b2", "validation_check_recorded"),
        entry("ev:c3", "validation_check_recorded"),
        entry("ev:d4", "validation_check_recorded"),
      ],
      3,
    );

    expect(groupKey(rows[0] as never)).toBe("ev:b2");
    expect(navigableEventIds(visualRows(rows, new Set()))).toEqual(["ev:b2"]);
    expect(navigableEventIds(visualRows(rows, new Set(["ev:b2"])))).toEqual([
      "ev:b2",
      "ev:c3",
      "ev:d4",
    ]);
  });

  it("keeps unchanged entry values inside expanded member rows", () => {
    const source = mixedPage();
    const rows = visualRows(
      groupTimelineEntries(source, 3),
      new Set(["ev:b2"]),
    );

    expect(
      rows.map((row) => (row.kind === "event" ? row.entry : null)),
    ).toEqual(source);
  });
});

describe("group ownership helpers", () => {
  it("reports the collapsed owner of a member and null for a flat row", () => {
    const rows = groupTimelineEntries(mixedPage(), 3);

    expect(owningGroupKey(rows, "ev:d4")).toBe("ev:b2");
    expect(owningGroupKey(rows, "ev:b2")).toBe("ev:b2");
    expect(owningGroupKey(rows, "ev:a1")).toBeNull();
    expect(owningGroupKey(rows, "ev:missing")).toBeNull();
  });

  it("reports a group only while it is collapsed", () => {
    const rows = groupTimelineEntries(mixedPage(), 3);

    expect(collapsedGroupAt(rows, "ev:b2", new Set())).toBe("ev:b2");
    expect(collapsedGroupAt(rows, "ev:b2", new Set(["ev:b2"]))).toBeNull();
    expect(collapsedGroupAt(rows, "ev:c3", new Set())).toBe("ev:b2");
    expect(collapsedGroupAt(rows, null, new Set())).toBeNull();
  });
});

describe("summarizeTimelineGroup", () => {
  const validation = (
    eventId: string,
    status: "passed" | "failed" | "errored" | "skipped",
    occurredAt: string,
  ): EventHistoryEntry =>
    ({
      eventId,
      eventType: "validation_check_recorded",
      occurredAt,
      summary: {
        kind: "validation_check_recorded",
        details: { status },
      },
    }) as unknown as EventHistoryEntry;

  it("composes count, first and last occurredAt, and a fixed-order status tally", () => {
    const group: TimelineGroup = {
      kind: "group",
      eventType: "validation_check_recorded",
      members: [
        validation("ev:a1", "failed", "2026-08-08T00:00:03Z"),
        validation("ev:b2", "passed", "2026-08-08T00:00:02Z"),
        validation("ev:c3", "skipped", "2026-08-08T00:00:01Z"),
        validation("ev:d4", "passed", "2026-08-08T00:00:00Z"),
      ],
    };

    expect(summarizeTimelineGroup(group)).toEqual({
      count: 4,
      firstOccurredAt: "2026-08-08T00:00:03Z",
      lastOccurredAt: "2026-08-08T00:00:00Z",
      statusTally: [
        { status: "passed", count: 2 },
        { status: "failed", count: 1 },
        { status: "skipped", count: 1 },
      ],
    });
  });

  it("reports no tally for a run that is not validations", () => {
    const group: TimelineGroup = {
      kind: "group",
      eventType: "review_observation_recorded",
      members: [
        entry("ev:a1", "review_observation_recorded"),
        entry("ev:b2", "review_observation_recorded"),
        entry("ev:c3", "review_observation_recorded"),
      ],
    };

    expect(summarizeTimelineGroup(group).statusTally).toEqual([]);
  });
});
