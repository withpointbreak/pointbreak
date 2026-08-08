import { describe, expect, it } from "vitest";
import { createTimelineMonitor } from "../src/change-inspector-timeline-monitor";
import type { EventHistoryDocument } from "../src/change-protocol";
import { authorityCursor } from "./support/authority";

function history(
  eventCount: number,
  matchCount: number,
  stamp: string,
  eventIds: string[] = [],
): EventHistoryDocument {
  return {
    schema: "pointbreak.inspect-event-history",
    version: 1,
    authorityCursor: authorityCursor(eventCount),
    sourceChangeProjectionStamp: `sha256:changes-${stamp}`,
    timelineProjectionStamp: `sha256:timeline-${stamp}`,
    order: "desc",
    eventCount,
    matchCount,
    offset: 0,
    facets: {},
    completion: {
      eventTypes: [],
      trackIds: [],
      changeIds: [],
      revisionRefs: [],
      unresolvedRevisionIds: [],
    },
    diagnostics: [],
    queryNotices: [],
    entries: eventIds.map(
      (eventId): EventHistoryDocument["entries"][number] => ({
        eventId,
        eventType: "review_note_imported",
        occurredAt: "2026-08-08T00:00:00Z",
        payloadHash: `sha256:payload-${eventId}`,
        journalId: "journal:sha256:test",
        trackId: undefined,
        writer: {
          actorId: "actor:test",
          producer: { name: "pointbreak", version: "0.10.0" },
        },
        verificationStatus: "valid",
        assertionMode: "advisory",
        subject: { kind: "journal", journalId: "journal:sha256:test" },
        changeIds: [],
        revisionRefs: [],
        unresolvedRevisionIds: [],
        summary: { kind: "review_note_imported" },
      }),
    ),
  };
}

const headRoute = {
  kind: "timeline" as const,
  historyQuery: { q: "assessment" },
};

describe("Timeline session monitor", () => {
  it("parks the filtered window and advertises only filtered matching appends", () => {
    const monitor = createTimelineMonitor();
    const initial = history(10, 4, "initial", ["evt:old", "evt:older"]);
    expect(monitor.observe(headRoute, initial)).toMatchObject({
      mode: "following",
      newCount: 0,
      display: initial,
    });

    expect(monitor.toggle()).toMatchObject({ mode: "parked", newCount: 0 });
    const matchingAppend = history(13, 6, "matching-append", [
      "evt:newest",
      "evt:newer",
      "evt:old",
    ]);
    expect(monitor.observe(headRoute, matchingAppend)).toMatchObject({
      mode: "parked",
      newCount: 2,
      display: initial,
    });

    expect(monitor.toggle()).toMatchObject({
      mode: "following",
      newCount: 0,
      display: matchingAppend,
    });

    // An authority/projection advance caused by an excluded append is real,
    // but it is not a new result for this filtered Timeline.
    const excludedMonitor = createTimelineMonitor();
    excludedMonitor.observe(headRoute, initial);
    excludedMonitor.toggle();
    const excludedAppend = history(11, 4, "excluded-append", [
      "evt:old",
      "evt:older",
    ]);
    expect(excludedMonitor.observe(headRoute, excludedAppend)).toMatchObject({
      mode: "parked",
      newCount: 0,
      display: initial,
    });
    expect(excludedMonitor.toggle()).toMatchObject({
      mode: "following",
      newCount: 0,
      display: excludedAppend,
    });
  });

  it("does not follow a continuation, historical anchor, or ascending view", () => {
    const monitor = createTimelineMonitor();
    expect(
      monitor.observe(
        { kind: "timeline", historyQuery: { after: "opaque" } },
        history(3, 3, "page-two"),
      ),
    ).toBeNull();
    expect(monitor.snapshot()).toBeNull();
    expect(
      monitor.observe(
        { kind: "timeline", historyQuery: { at: "evt:sha256:one" } },
        history(3, 3, "anchored"),
      ),
    ).toBeNull();
    expect(
      monitor.observe(
        { kind: "timeline", historyQuery: { order: "asc" } },
        history(3, 3, "ascending"),
      ),
    ).toBeNull();
  });

  it("uses retained event identity when the matching count does not change", () => {
    const monitor = createTimelineMonitor();
    const parked = history(10, 4, "parked", ["evt:old", "evt:older"]);
    monitor.observe(headRoute, parked);
    monitor.park();

    const changedHead = history(10, 4, "changed-head", ["evt:new", "evt:old"]);
    expect(monitor.observe(headRoute, changedHead)).toMatchObject({
      mode: "parked",
      newCount: 1,
      display: parked,
    });
  });

  it("parks idempotently and follows only when explicitly requested", () => {
    const monitor = createTimelineMonitor();
    const initial = history(3, 3, "initial", ["evt:one"]);
    monitor.observe(headRoute, initial);
    expect(monitor.park()).toMatchObject({ mode: "parked", display: initial });
    expect(monitor.park()).toMatchObject({ mode: "parked", display: initial });
    expect(monitor.follow()).toMatchObject({
      mode: "following",
      display: initial,
    });
  });
});
