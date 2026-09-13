import { describe, expect, it } from "vitest";
import {
  collapsedGroupAt,
  GROUP_MIN_RUN,
  groupKey,
  groupTimelineEntries,
  navigableEventIds,
  owningGroupKey,
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
