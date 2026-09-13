import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installChangeInspectorInteraction } from "../src/change-inspector-interaction";
import type { ChangeInspectorRoute } from "../src/change-inspector-router";
import type { ChangeInspectorSnapshot } from "../src/change-inspector-state";
import {
  renderChangeInspectorTimeline,
  revealChangeInspectorTimelineEvent,
  setChangeInspectorTimelineGroupExpanded,
} from "../src/change-inspector-timeline";
import type {
  EventHistoryDocument,
  EventHistoryEntry,
} from "../src/change-protocol";
import { authorityCursor } from "./support/authority";
import { mountInspectorDom, resetDom } from "./support/dom";

const route: Extract<ChangeInspectorRoute, { kind: "timeline" }> = {
  kind: "timeline",
  historyQuery: {},
};

const snapshot = (): ChangeInspectorSnapshot => ({
  generation: null,
  route,
  selected: null,
  diagnostic: null,
});

function entry(
  eventId: string,
  eventType: "review_note_imported" | "review_initialized",
): EventHistoryEntry {
  return {
    eventId,
    eventType,
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
    summary: { kind: eventType },
  };
}

/** ev:a1, then a collapsed run (ev:b2, ev:c3, ev:d4), then ev:e5. */
function groupedDocument(
  projectionStamp = "sha256:timeline",
): EventHistoryDocument {
  const entries = [
    entry("ev:a1", "review_initialized"),
    entry("ev:b2", "review_note_imported"),
    entry("ev:c3", "review_note_imported"),
    entry("ev:d4", "review_note_imported"),
    entry("ev:e5", "review_initialized"),
  ];
  return {
    schema: "pointbreak.inspect-event-history",
    version: 1,
    authorityCursor: authorityCursor(entries.length),
    sourceChangeProjectionStamp: "sha256:changes",
    timelineProjectionStamp: projectionStamp,
    order: "desc",
    eventCount: entries.length,
    matchCount: entries.length,
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
    entries,
  };
}

const activeControllers: Array<{ stop(): void }> = [];

function install() {
  const navigate = vi.fn();
  const replace = vi.fn();
  const controller = installChangeInspectorInteraction({
    navigate,
    replace,
    revealTimelineEvent: revealChangeInspectorTimelineEvent,
    expandTimelineGroup: (groupKey: string) =>
      setChangeInspectorTimelineGroupExpanded(groupKey, true),
    collapseTimelineGroup: (groupKey: string) =>
      setChangeInspectorTimelineGroupExpanded(groupKey, false),
  });
  activeControllers.push(controller);
  return { controller, navigate, replace };
}

function renderGroupedTimeline(projectionStamp?: string): EventHistoryDocument {
  const master = document.querySelector<HTMLElement>("#master");
  if (!master) throw new Error("missing master");
  const timeline = groupedDocument(projectionStamp);
  renderChangeInspectorTimeline(master, timeline, { navigate: vi.fn() }, route);
  return timeline;
}

function pressKey(key: string): void {
  const list = document.querySelector<HTMLOListElement>("#timeline");
  if (!list) throw new Error("missing Timeline list");
  list.focus();
  list.dispatchEvent(
    new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
  );
}

function selectedTimelineEventId(): string | null {
  return (
    document.querySelector<HTMLElement>('#timeline [aria-selected="true"]')
      ?.dataset.eventId ?? null
  );
}

function clickRow(eventId: string): void {
  document
    .querySelector<HTMLElement>(`#timeline [data-event-id="${eventId}"]`)
    ?.click();
}

beforeEach(() => {
  mountInspectorDom();
  vi.spyOn(window, "matchMedia").mockImplementation(
    (query: string) =>
      ({
        matches: false,
        media: query,
        onchange: null,
        addListener: vi.fn(),
        removeListener: vi.fn(),
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(() => true),
      }) as unknown as MediaQueryList,
  );
});

afterEach(() => {
  for (const controller of activeControllers.splice(0)) controller.stop();
  vi.restoreAllMocks();
  resetDom();
});

describe("grouped Timeline navigation", () => {
  it("walks only visible rows while a group is collapsed", () => {
    const { controller } = install();
    const timeline = renderGroupedTimeline();
    controller.sync(snapshot(), timeline);

    clickRow("ev:a1");
    expect(selectedTimelineEventId()).toBe("ev:a1");
    pressKey("j");
    expect(selectedTimelineEventId()).toBe("ev:b2");
    pressKey("j");
    // ev:c3 and ev:d4 are collapsed members and not navigable.
    expect(selectedTimelineEventId()).toBe("ev:e5");
    pressKey("k");
    expect(selectedTimelineEventId()).toBe("ev:b2");
  });

  it("expands a group with Enter and moves selection to its first member", () => {
    const { controller, navigate } = install();
    const timeline = renderGroupedTimeline();
    controller.sync(snapshot(), timeline);

    clickRow("ev:b2");
    const navigatedByClick = navigate.mock.calls.length;
    pressKey("Enter");

    // Enter on a collapsed group expands; it does not descend to detail.
    expect(navigate).toHaveBeenCalledTimes(navigatedByClick);
    expect(document.querySelector("#timeline [role='group']")).not.toBeNull();
    expect(selectedTimelineEventId()).toBe("ev:b2");
    pressKey("j");
    expect(selectedTimelineEventId()).toBe("ev:c3");
    pressKey("j");
    expect(selectedTimelineEventId()).toBe("ev:d4");
    pressKey("j");
    expect(selectedTimelineEventId()).toBe("ev:e5");
  });

  it("keeps Enter on a plain event row as descend-to-detail", () => {
    const { controller, navigate } = install();
    const timeline = renderGroupedTimeline();
    controller.sync(snapshot(), timeline);

    clickRow("ev:a1");
    // A pointer activation already navigated once; the key must navigate again.
    expect(navigate).toHaveBeenCalledTimes(1);
    pressKey("Enter");

    expect(navigate).toHaveBeenCalledTimes(2);
    expect(navigate).toHaveBeenLastCalledWith({
      kind: "event",
      eventId: "ev:a1",
      historyQuery: {},
      query: {},
    });
  });

  it("expands with ArrowRight and collapses with ArrowLeft back onto the group row", () => {
    const { controller } = install();
    const timeline = renderGroupedTimeline();
    controller.sync(snapshot(), timeline);

    clickRow("ev:b2");
    pressKey("ArrowRight");
    expect(document.querySelector("#timeline [role='group']")).not.toBeNull();
    clickRow("ev:d4");
    expect(selectedTimelineEventId()).toBe("ev:d4");

    pressKey("ArrowLeft");

    expect(selectedTimelineEventId()).toBe("ev:b2");
    expect(document.querySelector("#timeline [role='group']")).toBeNull();
    pressKey("j");
    expect(selectedTimelineEventId()).toBe("ev:e5");
  });

  it("ignores ArrowLeft and ArrowRight on a row that belongs to no group", () => {
    const { controller, navigate } = install();
    const timeline = renderGroupedTimeline();
    controller.sync(snapshot(), timeline);

    clickRow("ev:a1");
    pressKey("ArrowRight");
    pressKey("ArrowLeft");

    expect(selectedTimelineEventId()).toBe("ev:a1");
    expect(document.querySelector("#timeline [role='group']")).toBeNull();
    expect(navigate).toHaveBeenCalledTimes(1);
  });

  it("reveals a collapsed member by expanding its owning group first", () => {
    renderGroupedTimeline();

    expect(revealChangeInspectorTimelineEvent("ev:d4")).toBe(true);

    expect(document.querySelector("#timeline [role='group']")).not.toBeNull();
    const revealed = document.querySelector<HTMLElement>(
      '#timeline [data-event-id="ev:d4"]',
    );
    expect(revealed).not.toBeNull();
    expect(revealed?.getAttribute("aria-selected")).toBe("true");
  });

  it("lands a deep link on a collapsed member with its group opened", () => {
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");

    renderChangeInspectorTimeline(
      master,
      groupedDocument(),
      { navigate: vi.fn() },
      route,
      "ev:c3",
    );

    const list = document.querySelector<HTMLOListElement>("#timeline");
    const selected = list?.querySelector<HTMLElement>(
      '[data-event-id="ev:c3"]',
    );
    expect(selected?.getAttribute("aria-selected")).toBe("true");
    expect(list?.getAttribute("aria-activedescendant")).toBe(selected?.id);
    expect(list?.querySelector("[role='group']")).not.toBeNull();
  });

  it("refreshes the cursor sequence after a reveal expands a group", () => {
    const { controller } = install();
    const timeline = renderGroupedTimeline();
    controller.sync(snapshot(), timeline);

    clickRow("ev:a1");
    expect(revealChangeInspectorTimelineEvent("ev:d4")).toBe(true);
    controller.sync(snapshot(), timeline);
    pressKey("j");

    expect(selectedTimelineEventId()).toBe("ev:b2");
    pressKey("j");
    expect(selectedTimelineEventId()).toBe("ev:c3");
  });

  it("resets expansion when the render key changes", () => {
    const { controller } = install();
    controller.sync(snapshot(), renderGroupedTimeline());

    clickRow("ev:b2");
    pressKey("ArrowRight");
    expect(document.querySelector("#timeline [role='group']")).not.toBeNull();

    controller.sync(snapshot(), renderGroupedTimeline("sha256:timeline-2"));

    expect(document.querySelector("#timeline [role='group']")).toBeNull();
    expect(
      Array.from(
        document.querySelectorAll<HTMLElement>("#timeline [data-event-id]"),
      ).map((row) => row.dataset.eventId),
    ).toEqual(["ev:a1", "ev:b2", "ev:e5"]);
  });

  it("keeps expansion out of the URL and localStorage", () => {
    const { controller } = install();
    controller.sync(snapshot(), renderGroupedTimeline());
    const hash = location.hash;

    clickRow("ev:b2");
    pressKey("ArrowRight");

    expect(location.hash).toBe(hash);
    expect(localStorage.length).toBe(0);
  });
});
