import { describe, expect, it, vi } from "vitest";
import { ChangeInspectorGenerationChanged } from "../src/change-inspector-state";
import {
  ChangeInspectorTimelineTraversalRefused,
  firstTimelineRoute,
  traverseTimelineTail,
} from "../src/change-inspector-timeline-boundary";
import type {
  EventHistoryDocument,
  EventHistoryQuery,
} from "../src/change-protocol";
import { authorityCursor } from "./support/authority";

function page(
  offset: number,
  ids: string[],
  next?: string,
  overrides: Partial<EventHistoryDocument> = {},
): EventHistoryDocument {
  return {
    schema: "pointbreak.inspect-event-history",
    version: 1,
    authorityCursor: authorityCursor(5),
    sourceChangeProjectionStamp: "sha256:changes",
    timelineProjectionStamp: "sha256:timeline",
    order: "asc",
    eventCount: 5,
    matchCount: 5,
    offset,
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
    entries: ids.map((eventId) => ({
      eventId,
      eventType: "review_note_imported",
      occurredAt: "2026-08-08T00:00:00Z",
      payloadHash: `sha256:${eventId}`,
      journalId: "journal:sha256:test",
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
    })),
    next,
    ...overrides,
  };
}

const route = {
  kind: "timeline" as const,
  historyQuery: {
    q: "accepted",
    type: "review_assessment_recorded",
    track: "reviewer",
    change: "change:sha256:one",
    order: "asc" as const,
    limit: 2,
    after: "current-page",
  },
};

describe("global Timeline boundaries", () => {
  it("strips only positioning when returning to the filtered head", () => {
    expect(
      firstTimelineRoute({
        ...route,
        historyQuery: {
          ...route.historyQuery,
          at: "evt:one",
          after: undefined,
        },
      }),
    ).toEqual({
      kind: "timeline",
      historyQuery: {
        q: "accepted",
        type: "review_assessment_recorded",
        track: "reviewer",
        change: "change:sha256:one",
        order: "asc",
        limit: 2,
        after: undefined,
        at: undefined,
      },
    });
  });

  it("follows only server-issued opaque continuations to the real tail", async () => {
    const first = page(0, ["evt:one", "evt:two"], "opaque-a");
    const middle = page(2, ["evt:three", "evt:four"], "opaque-b");
    const last = page(4, ["evt:five"]);
    const requests: EventHistoryQuery[] = [];
    const load = vi.fn(async (query: EventHistoryQuery) => {
      requests.push(query);
      if (query.after === undefined) return first;
      if (query.after === "opaque-a") return middle;
      if (query.after === "opaque-b") return last;
      throw new Error("invented continuation");
    });

    await expect(traverseTimelineTail(route, first, load)).resolves.toEqual({
      route: {
        kind: "timeline",
        historyQuery: {
          q: "accepted",
          type: "review_assessment_recorded",
          track: "reviewer",
          change: "change:sha256:one",
          order: "asc",
          limit: 2,
          after: "opaque-b",
        },
      },
      page: last,
      pageCount: 3,
    });
    expect(requests.map((request) => request.after)).toEqual([
      undefined,
      "opaque-a",
      "opaque-b",
    ]);
  });

  it("refuses a continuation cycle within the match-count bound", async () => {
    const first = page(0, ["evt:one", "evt:two"], "opaque-a");
    const looping = page(2, ["evt:three", "evt:four"], "opaque-a");
    await expect(
      traverseTimelineTail(route, first, async (query) =>
        query.after === undefined ? first : looping,
      ),
    ).rejects.toBeInstanceOf(ChangeInspectorTimelineTraversalRefused);
  });

  it("refuses a page from a moving projection generation", async () => {
    const first = page(0, ["evt:one", "evt:two"], "opaque-a");
    const stale = page(2, ["evt:three", "evt:four"], undefined, {
      timelineProjectionStamp: "sha256:moved",
    });
    await expect(
      traverseTimelineTail(route, first, async (query) =>
        query.after === undefined ? first : stale,
      ),
    ).rejects.toBeInstanceOf(ChangeInspectorGenerationChanged);
  });
});
