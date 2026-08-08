import { describe, expect, it } from "vitest";
import {
  boundaryTimelineSelection,
  moveTimelineSelection,
  pageTimelineSelection,
  resolveTimelinePageSelection,
} from "../src/change-inspector-timeline-navigation";

const window = (selectedEventId: string | null = "evt:two") => ({
  eventIds: ["evt:one", "evt:two", "evt:three"],
  selectedEventId,
});

describe("bounded Timeline navigation", () => {
  it("moves inside the rendered event window without choosing a Change or Revision", () => {
    expect(moveTimelineSelection(window(), 1)).toEqual({
      kind: "select",
      eventId: "evt:three",
    });
    expect(moveTimelineSelection(window(), -1)).toEqual({
      kind: "select",
      eventId: "evt:one",
    });
  });

  it("turns an edge move into an adjacent signed-page request with a landing anchor", () => {
    expect(moveTimelineSelection(window("evt:three"), 1)).toEqual({
      kind: "adjacent-page",
      direction: "next",
      index: 0,
    });
    expect(moveTimelineSelection(window("evt:one"), -1)).toEqual({
      kind: "adjacent-page",
      direction: "previous",
      indexFromEnd: 0,
    });
    expect(
      resolveTimelinePageSelection(["evt:four", "evt:five"], {
        kind: "adjacent-page",
        direction: "next",
        index: 0,
      }),
    ).toBe("evt:four");
    expect(
      resolveTimelinePageSelection(["evt:previous", "evt:one"], {
        kind: "adjacent-page",
        direction: "previous",
        indexFromEnd: 0,
      }),
    ).toBe("evt:one");
  });

  it("uses full and half viewport movement while retaining a bounded landing position", () => {
    expect(
      pageTimelineSelection(window("evt:two"), "forward", 2, "full"),
    ).toEqual({ kind: "adjacent-page", direction: "next", index: 0 });
    expect(
      pageTimelineSelection(window("evt:two"), "backward", 2, "half"),
    ).toEqual({ kind: "select", eventId: "evt:one" });
  });

  it("uses the visible first and last event for g/G without inventing an absent global offset", () => {
    expect(boundaryTimelineSelection(window(), "first")).toEqual({
      kind: "select",
      eventId: "evt:one",
    });
    expect(boundaryTimelineSelection(window(), "last")).toEqual({
      kind: "select",
      eventId: "evt:three",
    });
  });
});
