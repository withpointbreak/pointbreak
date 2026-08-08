import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { promptForCredential } from "../src/auth";
import { installChangeInspectorInteraction } from "../src/change-inspector-interaction";
import {
  type ChangeInspectorRoute,
  formatChangeInspectorRoute,
} from "../src/change-inspector-router";
import type { ChangeInspectorSnapshot } from "../src/change-inspector-state";
import {
  renderChangeInspectorTimeline,
  revealChangeInspectorTimelineEvent,
} from "../src/change-inspector-timeline";
import type { EventHistoryDocument } from "../src/change-protocol";
import { authorityCursor } from "./support/authority";
import { mountInspectorDom, resetDom } from "./support/dom";

const lensSnapshot = (): ChangeInspectorSnapshot => ({
  generation: null,
  route: { kind: "lens", lens: "changes", query: {} },
  selected: null,
  diagnostic: null,
});

const attentionSnapshot = (): ChangeInspectorSnapshot => ({
  generation: null,
  route: { kind: "lens", lens: "attention", query: { q: "needs-review" } },
  selected: null,
  diagnostic: null,
});

const timelineSnapshot = (): ChangeInspectorSnapshot => ({
  generation: null,
  route: {
    kind: "timeline",
    historyQuery: {
      q: "assessment",
      type: "assessment_recorded",
      after: "opaque-continuation",
    },
  },
  selected: null,
  diagnostic: null,
});

const exactSnapshot = (
  changeId = "change:sha256:interaction-lifecycle",
  query: Record<string, string> = {},
): ChangeInspectorSnapshot => ({
  generation: null,
  route: {
    kind: "change",
    changeId,
    query,
  },
  selected: null,
  diagnostic: null,
});

const eventSnapshot = (): ChangeInspectorSnapshot => ({
  generation: null,
  route: {
    kind: "event",
    eventId: "evt:sha256:deep-link",
    historyQuery: {
      q: "assessment",
      track: "reviewer",
      after: "opaque-event-page",
    },
    query: {},
  },
  selected: null,
  diagnostic: null,
});

const activeControllers: Array<{ stop(): void }> = [];

function timelineDocument(eventIds: string[]): EventHistoryDocument {
  return {
    schema: "pointbreak.inspect-event-history",
    version: 1,
    authorityCursor: authorityCursor(eventIds.length),
    sourceChangeProjectionStamp: "sha256:changes",
    timelineProjectionStamp: "sha256:timeline",
    order: "desc",
    eventCount: eventIds.length,
    matchCount: eventIds.length,
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
    entries: eventIds.map((eventId) => ({
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
  };
}

function install() {
  const navigate = vi.fn();
  const toggleTimelineMonitoring = vi.fn();
  const controller = installChangeInspectorInteraction({
    navigate,
    toggleTimelineMonitoring,
  });
  activeControllers.push(controller);
  return { controller, navigate, toggleTimelineMonitoring };
}

function mountTimelineRows(
  eventIds: string[],
  route?: Extract<ChangeInspectorRoute, { kind: "timeline" }>,
): HTMLOListElement {
  const list = document.createElement("ol");
  list.id = "timeline";
  list.tabIndex = 0;
  const mountedRoute = route ?? timelineSnapshot().route;
  if (mountedRoute.kind !== "timeline") {
    throw new Error("Timeline row fixture requires a Timeline route");
  }
  list.dataset.timelineRoute = formatChangeInspectorRoute(mountedRoute);
  for (const eventId of eventIds) {
    const row = document.createElement("li");
    row.className = "event";
    row.dataset.eventId = eventId;
    row.tabIndex = 0;
    row.textContent = eventId;
    list.append(row);
  }
  document.querySelector("#master")?.append(list);
  return list;
}

beforeEach(() => {
  localStorage.clear();
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

describe("Change Inspector interaction lifecycle", () => {
  it("reserves Shift-F for Timeline follow while text controls own their keys", () => {
    const { controller, toggleTimelineMonitoring } = install();
    controller.sync(timelineSnapshot(), timelineDocument([]));
    const input = document.querySelector<HTMLInputElement>("#filter-text");
    input?.focus();
    input?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "F", bubbles: true }),
    );
    expect(toggleTimelineMonitoring).not.toHaveBeenCalled();

    document.body.focus();
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "F", bubbles: true }),
    );
    expect(toggleTimelineMonitoring).toHaveBeenCalledOnce();
  });

  it("keeps one Timeline tab stop and moves its event cursor with j/k/g/G", () => {
    const { controller, navigate } = install();
    const list = mountTimelineRows(["evt:one", "evt:two", "evt:three"]);
    controller.sync(
      timelineSnapshot(),
      timelineDocument(["evt:one", "evt:two", "evt:three"]),
    );

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true }),
    );
    expect(list.getAttribute("aria-activedescendant")).toContain("evt_3Aone");
    expect(document.activeElement).toBe(list);
    expect(
      Array.from(list.querySelectorAll<HTMLElement>("[data-event-id]")).map(
        (row) => row.tabIndex,
      ),
    ).toEqual([-1, -1, -1]);

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "G", bubbles: true }),
    );
    expect(list.getAttribute("aria-activedescendant")).toContain("evt_3Athree");
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "k", bubbles: true }),
    );
    expect(list.getAttribute("aria-activedescendant")).toContain("evt_3Atwo");
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "g", bubbles: true }),
    );
    expect(list.getAttribute("aria-activedescendant")).toContain("evt_3Aone");
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    expect(navigate).toHaveBeenLastCalledWith({
      kind: "event",
      eventId: "evt:one",
      historyQuery: {
        q: "assessment",
        type: "assessment_recorded",
        after: "opaque-continuation",
      },
      query: {},
    });
  });

  it("opens a clicked Timeline event exactly once through the interaction owner", () => {
    const { controller, navigate } = install();
    const timeline = timelineDocument(["evt:one", "evt:two"]);
    const route = timelineSnapshot().route;
    const master = document.querySelector<HTMLElement>("#master");
    if (!master || route.kind !== "timeline") {
      throw new Error("Timeline interaction fixture is incomplete");
    }
    renderChangeInspectorTimeline(master, timeline, { navigate }, route);
    controller.sync(timelineSnapshot(), timeline);

    document
      .querySelector<HTMLElement>('#timeline [data-event-id="evt:one"]')
      ?.click();

    expect(navigate).toHaveBeenCalledOnce();
    expect(navigate).toHaveBeenCalledWith({
      kind: "event",
      eventId: "evt:one",
      historyQuery: {
        q: "assessment",
        type: "assessment_recorded",
        after: undefined,
      },
      query: {},
    });
    expect(
      document
        .querySelector<HTMLOListElement>("#timeline")
        ?.getAttribute("aria-activedescendant"),
    ).toContain("evt_3Aone");
  });

  it("lands a global boundary selection only after its routed page paints", async () => {
    const navigate = vi.fn();
    const target = {
      kind: "timeline" as const,
      historyQuery: {
        q: "assessment",
        type: "assessment_recorded",
        after: "opaque-tail",
      },
    };
    const navigateTimelineBoundary = vi.fn(async () => target);
    const controller = installChangeInspectorInteraction({
      navigate,
      navigateTimelineBoundary,
    });
    activeControllers.push(controller);
    const list = mountTimelineRows(["evt:one", "evt:two"]);
    controller.sync(
      timelineSnapshot(),
      timelineDocument(["evt:one", "evt:two"]),
    );
    list.focus();

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "G", bubbles: true }),
    );
    await Promise.resolve();
    expect(navigateTimelineBoundary).toHaveBeenCalledWith(
      "last",
      timelineSnapshot().route,
    );
    expect(list.getAttribute("aria-activedescendant")).toBeNull();

    // A route can become current before its destination DOM replaces the old
    // page. Presence alone cannot authorize consuming the pending tail intent:
    // the mounted list must identify the same exact Timeline route.
    controller.sync(
      {
        ...timelineSnapshot(),
        route: target,
      },
      timelineDocument(["evt:one", "evt:two"]),
    );
    expect(list.getAttribute("aria-activedescendant")).toBeNull();

    list.remove();
    const tailList = mountTimelineRows(
      ["evt:tail-one", "evt:tail-two"],
      target,
    );
    controller.sync(
      {
        ...timelineSnapshot(),
        route: target,
      },
      timelineDocument(["evt:tail-one", "evt:tail-two"]),
    );
    expect(tailList.getAttribute("aria-activedescendant")).toContain(
      "evt_3Atail-two",
    );
    expect(document.activeElement).toBe(tailList);
  });

  it("keeps an exact-event Timeline keyboard-active without changing its detail until Enter", () => {
    const { controller, navigate } = install();
    const snapshot = eventSnapshot();
    if (snapshot.route.kind !== "event") throw new Error("missing event route");
    const companion = {
      kind: "timeline" as const,
      historyQuery: snapshot.route.historyQuery,
    };
    const list = mountTimelineRows(
      [snapshot.route.eventId, "evt:sha256:next"],
      companion,
    );
    const nextPage = vi.fn();
    const next = document.createElement("button");
    next.dataset.timelinePage = "next";
    next.dataset.timelineTargetRoute = "#/timeline?after=next-page";
    next.addEventListener("click", nextPage);
    const previousPage = vi.fn();
    const previous = document.createElement("button");
    previous.dataset.timelinePage = "previous";
    previous.dataset.timelineTargetRoute = "#/timeline?after=previous-page";
    previous.addEventListener("click", previousPage);
    document.querySelector("#master")?.append(previous, next);
    list.focus();
    controller.sync(
      snapshot,
      timelineDocument([snapshot.route.eventId, "evt:sha256:next"]),
    );
    expect(list.getAttribute("aria-activedescendant")).toContain(
      "evt_3Asha256_3Adeep-link",
    );

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "G", bubbles: true }),
    );
    expect(list.getAttribute("aria-activedescendant")).toContain(
      "evt_3Asha256_3Adeep-link",
    );
    expect(navigate).not.toHaveBeenCalled();
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "g", bubbles: true }),
    );
    expect(list.getAttribute("aria-activedescendant")).toContain(
      "evt_3Asha256_3Adeep-link",
    );

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true }),
    );
    expect(list.getAttribute("aria-activedescendant")).toContain(
      "evt_3Asha256_3Anext",
    );
    expect(navigate).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(list);
    for (const key of ["j", "f", "d"]) {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key, bubbles: true }),
      );
    }
    expect(nextPage).not.toHaveBeenCalled();
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "k", bubbles: true }),
    );
    for (const key of ["k", "b", "u"]) {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key, bubbles: true }),
      );
    }
    expect(previousPage).not.toHaveBeenCalled();
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true }),
    );

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    expect(navigate).toHaveBeenCalledWith({
      kind: "event",
      eventId: "evt:sha256:next",
      historyQuery: snapshot.route.historyQuery,
      query: {},
    });
  });

  it("leaves native detail arrows alone on an exact-event route", () => {
    const { controller } = install();
    const snapshot = eventSnapshot();
    if (snapshot.route.kind !== "event") throw new Error("missing event route");
    const list = mountTimelineRows(
      [snapshot.route.eventId, "evt:sha256:next"],
      { kind: "timeline", historyQuery: snapshot.route.historyQuery },
    );
    controller.sync(
      snapshot,
      timelineDocument([snapshot.route.eventId, "evt:sha256:next"]),
    );
    const close = document.querySelector<HTMLButtonElement>("#detail-close");
    close?.focus();
    const detailArrow = new KeyboardEvent("keydown", {
      key: "ArrowDown",
      bubbles: true,
      cancelable: true,
    });
    close?.dispatchEvent(detailArrow);
    expect(detailArrow.defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(close);
    expect(list.getAttribute("aria-activedescendant")).toContain(
      "evt_3Asha256_3Adeep-link",
    );

    list.focus();
    const timelineArrow = new KeyboardEvent("keydown", {
      key: "ArrowDown",
      bubbles: true,
      cancelable: true,
    });
    list.dispatchEvent(timelineArrow);
    expect(timelineArrow.defaultPrevented).toBe(true);
    expect(list.getAttribute("aria-activedescendant")).toContain(
      "evt_3Asha256_3Anext",
    );
  });

  it("leaves Enter on an exact-event disclosure to the native summary", () => {
    const { controller, navigate } = install();
    const snapshot = eventSnapshot();
    if (snapshot.route.kind !== "event") throw new Error("missing event route");
    mountTimelineRows([snapshot.route.eventId, "evt:sha256:next"], {
      kind: "timeline",
      historyQuery: snapshot.route.historyQuery,
    });
    controller.sync(
      snapshot,
      timelineDocument([snapshot.route.eventId, "evt:sha256:next"]),
    );
    const details = document.createElement("details");
    const summary = document.createElement("summary");
    summary.textContent = "Structured event data";
    details.append(summary, document.createElement("pre"));
    document.querySelector("#detail-body")?.append(details);
    summary.focus();
    const enter = new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      cancelable: true,
    });
    summary.dispatchEvent(enter);

    expect(enter.defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(summary);
    expect(navigate).not.toHaveBeenCalled();
  });

  it("keeps a narrow exact-event sheet isolated and repairs focus when widened", () => {
    let narrow = true;
    vi.spyOn(window, "matchMedia").mockImplementation(
      (query: string) =>
        ({
          matches: query === "(max-width: 760px)" && narrow,
          media: query,
          onchange: null,
          addListener: vi.fn(),
          removeListener: vi.fn(),
          addEventListener: vi.fn(),
          removeEventListener: vi.fn(),
          dispatchEvent: vi.fn(() => true),
        }) as unknown as MediaQueryList,
    );
    const { controller } = install();
    const snapshot = eventSnapshot();
    if (snapshot.route.kind !== "event") throw new Error("missing event route");
    const list = mountTimelineRows(
      [snapshot.route.eventId, "evt:sha256:next"],
      { kind: "timeline", historyQuery: snapshot.route.historyQuery },
    );
    controller.sync(
      snapshot,
      timelineDocument([snapshot.route.eventId, "evt:sha256:next"]),
    );
    const back = document.querySelector<HTMLButtonElement>("#detail-back");
    expect(document.activeElement).toBe(back);
    expect(document.querySelector<HTMLElement>("#master")?.inert).toBe(true);

    back?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true }),
    );
    expect(document.activeElement).toBe(back);
    expect(list.getAttribute("aria-activedescendant")).toContain(
      "evt_3Asha256_3Adeep-link",
    );
    const splitBefore =
      document.documentElement.style.getPropertyValue("--split-master");
    for (const key of ["/", "h", "l"]) {
      back?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
      expect(document.activeElement).toBe(back);
      expect(
        document.documentElement.style.getPropertyValue("--split-master"),
      ).toBe(splitBefore);
    }

    narrow = false;
    window.dispatchEvent(new Event("resize"));
    expect(document.activeElement).toBe(
      document.querySelector("#detail-close"),
    );
    expect(document.querySelector<HTMLElement>("#master")?.inert).toBe(false);
  });

  it("moves consecutively through a full 100-entry page beyond the mounted virtual slice", () => {
    const eventIds = Array.from({ length: 100 }, (_, index) => `evt:${index}`);
    const history = timelineDocument(eventIds);
    history.next = "opaque-next-page";
    history.matchCount = 101;
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    const navigate = vi.fn();
    renderChangeInspectorTimeline(
      master,
      history,
      { navigate },
      {
        kind: "timeline",
        historyQuery: {
          q: "assessment",
          type: "assessment_recorded",
          after: "opaque-continuation",
        },
      },
    );
    const list = document.querySelector<HTMLOListElement>("#timeline");
    if (!list) throw new Error("missing Timeline list");
    Object.defineProperty(list, "clientHeight", {
      configurable: true,
      value: 144,
    });
    revealChangeInspectorTimelineEvent("evt:0");
    expect(list.querySelectorAll("[data-event-id]").length).toBeLessThan(100);

    const controller = installChangeInspectorInteraction({
      navigate,
      revealTimelineEvent: revealChangeInspectorTimelineEvent,
    });
    activeControllers.push(controller);
    controller.sync(timelineSnapshot(), history);
    for (let index = 0; index < 20; index += 1) {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "j", bubbles: true }),
      );
    }

    expect(list.getAttribute("aria-activedescendant")).toContain("evt_3A19");
    expect(list.querySelector('[data-event-id="evt:19"]')).not.toBeNull();
    expect(navigate).not.toHaveBeenCalled();
  });

  it("uses f/b/u/d for Timeline movement and adjacent page controls at a boundary", () => {
    const { controller } = install();
    const sourceRoute = timelineSnapshot().route;
    if (sourceRoute.kind !== "timeline")
      throw new Error("missing source route");
    const nextRoute = {
      kind: "timeline" as const,
      historyQuery: { after: "next-page" },
    };
    const list = mountTimelineRows(["evt:one", "evt:two"]);
    Object.defineProperty(list, "clientHeight", {
      configurable: true,
      value: 1,
    });
    for (const row of list.querySelectorAll<HTMLElement>("[data-event-id]")) {
      row.getBoundingClientRect = () => ({ height: 1 }) as DOMRect;
    }
    const next = document.createElement("button");
    next.type = "button";
    next.dataset.timelinePage = "next";
    next.dataset.timelineTargetRoute = formatChangeInspectorRoute(nextRoute);
    next.textContent = "Next page";
    const nextPage = vi.fn();
    next.addEventListener("click", nextPage);
    const previous = document.createElement("button");
    previous.type = "button";
    previous.dataset.timelinePage = "previous";
    previous.dataset.timelineTargetRoute =
      formatChangeInspectorRoute(sourceRoute);
    previous.textContent = "Previous page";
    const previousPage = vi.fn();
    previous.addEventListener("click", previousPage);
    document.querySelector("#master")?.append(previous, next);
    controller.sync(
      timelineSnapshot(),
      timelineDocument(["evt:one", "evt:two"]),
    );
    list.focus();

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true }),
    );
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "f", bubbles: true }),
    );
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "f", bubbles: true }),
    );
    expect(nextPage).toHaveBeenCalledOnce();

    // Simulate the server-owned next page replacing the bounded DOM. The
    // pending anchor selects its first event and preserves focus on the list.
    list.remove();
    const nextList = mountTimelineRows(["evt:three", "evt:four"], nextRoute);
    Object.defineProperty(nextList, "clientHeight", {
      configurable: true,
      value: 1,
    });
    for (const row of nextList.querySelectorAll<HTMLElement>(
      "[data-event-id]",
    )) {
      row.getBoundingClientRect = () => ({ height: 1 }) as DOMRect;
    }
    controller.sync(
      {
        ...timelineSnapshot(),
        route: nextRoute,
      },
      timelineDocument(["evt:three", "evt:four"]),
    );
    expect(nextList.getAttribute("aria-activedescendant")).toContain(
      "evt_3Athree",
    );
    expect(document.activeElement).toBe(nextList);

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "u", bubbles: true }),
    );
    expect(nextList.getAttribute("aria-activedescendant")).toContain(
      "evt_3Athree",
    );
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "d", bubbles: true }),
    );
    expect(nextList.getAttribute("aria-activedescendant")).toContain(
      "evt_3Afour",
    );
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "b", bubbles: true }),
    );
    expect(nextList.getAttribute("aria-activedescendant")).toContain(
      "evt_3Athree",
    );
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "b", bubbles: true }),
    );
    // `u` at the first row and the final `b` at the first row are each one
    // deliberate adjacent-page action, so the adjacent signed page is asked
    // for twice rather than coalescing two independent reader keys.
    expect(previousPage).toHaveBeenCalledTimes(2);
  });

  it("discards an adjacent-page intent when a different Timeline route wins", () => {
    const { controller } = install();
    const source = timelineSnapshot().route;
    if (source.kind !== "timeline") throw new Error("missing source route");
    const target = {
      kind: "timeline" as const,
      historyQuery: { q: "assessment", after: "target-page" },
    };
    const unrelated = {
      kind: "timeline" as const,
      historyQuery: { q: "different-filter" },
    };
    const sourceList = mountTimelineRows(["evt:one"], source);
    const next = document.createElement("button");
    next.type = "button";
    next.dataset.timelinePage = "next";
    next.dataset.timelineTargetRoute = formatChangeInspectorRoute(target);
    document.querySelector("#master")?.append(next);
    controller.sync(timelineSnapshot(), timelineDocument(["evt:one"]));
    sourceList.focus();
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true }),
    );
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true }),
    );

    sourceList.remove();
    const unrelatedList = mountTimelineRows(["evt:other"], unrelated);
    controller.sync(
      { ...timelineSnapshot(), route: unrelated },
      timelineDocument(["evt:other"]),
    );
    expect(unrelatedList.getAttribute("aria-activedescendant")).toBeNull();

    unrelatedList.remove();
    const lateTarget = mountTimelineRows(["evt:late"], target);
    controller.sync(
      { ...timelineSnapshot(), route: target },
      timelineDocument(["evt:late"]),
    );
    expect(lateTarget.getAttribute("aria-activedescendant")).toBeNull();
    expect(document.activeElement).not.toBe(lateTarget);
  });

  it("returns an exact Revision opened from Timeline to its filtered Timeline route", () => {
    const { controller, navigate } = install();
    controller.sync(timelineSnapshot(), timelineDocument([]));
    controller.sync(exactSnapshot());

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );

    expect(navigate).toHaveBeenCalledWith({
      kind: "timeline",
      historyQuery: {
        q: "assessment",
        type: "assessment_recorded",
        after: "opaque-continuation",
      },
    });
  });

  it("returns a direct event deep link to its typed Timeline filter context", () => {
    const { controller, navigate } = install();
    controller.sync(eventSnapshot());

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );

    expect(navigate).toHaveBeenCalledWith({
      kind: "timeline",
      historyQuery: {
        q: "assessment",
        track: "reviewer",
        after: "opaque-event-page",
      },
    });
  });

  it("leaves Enter on a focused native control instead of opening the local Change cursor", () => {
    const { controller, navigate } = install();
    const card = document.createElement("article");
    card.className = "unit-card";
    card.dataset.changeId = "change:sha256:selected";
    const exactPeer = document.createElement("button");
    exactPeer.type = "button";
    exactPeer.textContent = "open exact Revision";
    card.append(exactPeer);
    document.querySelector("#master")?.append(card);
    controller.sync(lensSnapshot());

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true }),
    );
    exactPeer.focus();
    exactPeer.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(document.activeElement).toBe(exactPeer);
    expect(navigate).not.toHaveBeenCalled();
  });

  it("supports pointer resizing and double-click reset on the active split divider", () => {
    const { controller } = install();
    controller.sync(exactSnapshot());
    const split = document.querySelector<HTMLElement>(".split");
    const divider = document.querySelector<HTMLElement>(".divider");
    if (!split || !divider) throw new Error("split controls missing");
    vi.spyOn(split, "getBoundingClientRect").mockReturnValue({
      left: 0,
      width: 1000,
      top: 0,
      height: 300,
      right: 1000,
      bottom: 300,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    } as DOMRect);

    divider.dispatchEvent(
      new PointerEvent("pointerdown", {
        pointerId: 9,
        pointerType: "mouse",
        button: 2,
        bubbles: true,
      }),
    );
    expect(divider.classList).not.toContain("dragging");
    divider.dispatchEvent(
      new PointerEvent("pointerdown", { pointerId: 1, bubbles: true }),
    );
    expect(document.activeElement).toBe(divider);
    divider.dispatchEvent(
      new PointerEvent("pointermove", {
        pointerId: 2,
        clientX: 700,
        bubbles: true,
      }),
    );
    divider.dispatchEvent(
      new PointerEvent("pointerup", { pointerId: 2, bubbles: true }),
    );
    expect(
      document.documentElement.style.getPropertyValue("--split-master"),
    ).toBe("");
    expect(divider.classList).toContain("dragging");
    divider.dispatchEvent(
      new PointerEvent("pointermove", {
        pointerId: 1,
        clientX: 620,
        bubbles: true,
      }),
    );
    divider.dispatchEvent(
      new PointerEvent("pointerup", { pointerId: 1, bubbles: true }),
    );

    expect(
      document.documentElement.style.getPropertyValue("--split-master"),
    ).toBe("62%");
    expect(divider.getAttribute("aria-valuenow")).toBe("62");
    expect(divider.classList).not.toContain("dragging");

    divider.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    expect(
      document.documentElement.style.getPropertyValue("--split-master"),
    ).toBe("");
    expect(divider.getAttribute("aria-valuenow")).toBe("50");
  });

  it("cleans divider pointer capture on cancellation, loss, and controller stop", () => {
    const { controller } = install();
    controller.sync(exactSnapshot());
    const divider = document.querySelector<HTMLElement>(".divider");
    if (!divider) throw new Error("divider missing");
    const releasePointerCapture = vi.fn();
    Object.defineProperties(divider, {
      setPointerCapture: { configurable: true, value: vi.fn() },
      hasPointerCapture: {
        configurable: true,
        value: vi.fn(() => true),
      },
      releasePointerCapture: {
        configurable: true,
        value: releasePointerCapture,
      },
    });

    divider.dispatchEvent(
      new PointerEvent("pointerdown", { pointerId: 3, bubbles: true }),
    );
    divider.dispatchEvent(
      new PointerEvent("pointercancel", { pointerId: 3, bubbles: true }),
    );
    expect(divider.classList).not.toContain("dragging");
    expect(releasePointerCapture).toHaveBeenLastCalledWith(3);

    divider.dispatchEvent(
      new PointerEvent("pointerdown", { pointerId: 4, bubbles: true }),
    );
    divider.dispatchEvent(
      new PointerEvent("lostpointercapture", {
        pointerId: 4,
        bubbles: true,
      }),
    );
    expect(divider.classList).not.toContain("dragging");

    divider.dispatchEvent(
      new PointerEvent("pointerdown", { pointerId: 5, bubbles: true }),
    );
    controller.stop();
    expect(divider.classList).not.toContain("dragging");
    expect(releasePointerCapture).toHaveBeenLastCalledWith(5);
  });

  it("settles reconnect Escape through auth and restores the prior focus", async () => {
    const { controller, navigate } = install();
    controller.sync(lensSnapshot());
    const opener = document.createElement("button");
    opener.textContent = "retry connection";
    document.querySelector("#master")?.append(opener);
    opener.focus();

    const pending = promptForCredential();
    const input = document.querySelector<HTMLInputElement>("#reconnect-input");
    const submit =
      document.querySelector<HTMLButtonElement>("#reconnect-submit");
    expect(document.activeElement).toBe(input);

    submit?.focus();
    submit?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "1", bubbles: true }),
    );
    expect(navigate).not.toHaveBeenCalled();

    const tab = new KeyboardEvent("keydown", {
      key: "Tab",
      bubbles: true,
      cancelable: true,
    });
    submit?.dispatchEvent(tab);
    expect(tab.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(input);

    input?.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );

    await expect(pending).resolves.toBeNull();
    expect(
      document.querySelector("#reconnect-dialog")?.classList.contains("hidden"),
    ).toBe(true);
    expect(document.activeElement).toBe(opener);
    expect(navigate).not.toHaveBeenCalled();
  });

  it("leaves reading mode before an exact route returns focus to its lens", () => {
    const { controller } = install();
    controller.sync(lensSnapshot());
    const opener = document.createElement("button");
    opener.textContent = "open change";
    document.querySelector("#master")?.append(opener);
    opener.focus();

    controller.sync(exactSnapshot());
    document.querySelector<HTMLButtonElement>("#detail-read")?.click();
    expect(document.querySelector(".split")?.classList).toContain("reading");

    controller.sync(lensSnapshot());

    const split = document.querySelector(".split");
    expect(split?.classList).not.toContain("reading");
    expect(split?.classList).toContain("split-closed");
    expect(document.querySelector<HTMLElement>("#detail")?.inert).toBe(true);
    expect(document.activeElement).toBe(opener);
  });

  it("restores retained detail focus when an exact route replaces focused detail DOM", () => {
    const { controller } = install();
    history.replaceState(null, "", "/#/changes");
    controller.sync(lensSnapshot());
    history.pushState(null, "", "/#/changes/change%3Asha256%3Aone");
    controller.sync(exactSnapshot("change:sha256:one"));

    const oldDetailControl = document.createElement("button");
    oldDetailControl.textContent = "old exact action";
    document.querySelector("#detail-body")?.append(oldDetailControl);
    oldDetailControl.focus();
    expect(document.activeElement).toBe(oldDetailControl);

    history.pushState(null, "", "/#/changes/change%3Asha256%3Atwo");
    document.querySelector("#detail-body")?.replaceChildren();
    controller.sync(exactSnapshot("change:sha256:two"));

    expect(document.activeElement).toBe(
      document.querySelector("#detail-close"),
    );
  });

  it("moves focus into a new exact route when detail is already a narrow sheet", () => {
    let narrow = false;
    vi.spyOn(window, "matchMedia").mockImplementation(
      (query: string) =>
        ({
          matches: query === "(max-width: 760px)" && narrow,
          media: query,
          onchange: null,
          addListener: vi.fn(),
          removeListener: vi.fn(),
          addEventListener: vi.fn(),
          removeEventListener: vi.fn(),
          dispatchEvent: vi.fn(() => true),
        }) as unknown as MediaQueryList,
    );
    const { controller } = install();
    history.replaceState(null, "", "/#/changes");
    controller.sync(lensSnapshot());

    const opener = document.createElement("button");
    opener.textContent = "open change";
    document.querySelector("#master")?.append(opener);
    opener.focus();
    history.pushState(null, "", "/#/changes/change%3Asha256%3Awide");
    const exact = exactSnapshot("change:sha256:wide");
    controller.sync(exact);
    expect(document.activeElement).toBe(opener);

    const toolbarControl = document.querySelector<HTMLElement>("#view-toggle");
    toolbarControl?.focus();
    narrow = true;
    const nextGeneration = document.createElement("p");
    nextGeneration.textContent = "different exact detail";
    document.querySelector("#detail-body")?.replaceChildren(nextGeneration);
    history.pushState(null, "", "/#/changes/change%3Asha256%3Anarrow");
    const narrowExact = exactSnapshot("change:sha256:narrow");
    controller.sync(narrowExact);
    expect(document.activeElement).toBe(document.querySelector("#detail-back"));

    toolbarControl?.focus();
    const refreshedGeneration = document.createElement("p");
    refreshedGeneration.textContent = "same exact detail refresh";
    document
      .querySelector("#detail-body")
      ?.replaceChildren(refreshedGeneration);
    controller.sync(narrowExact);
    expect(document.activeElement).toBe(document.querySelector("#detail-back"));
    expect(document.querySelector<HTMLElement>("#topbar")?.inert).toBe(true);
  });

  it("restores focus after a same-route detail refresh but not an unchanged paint", () => {
    const { controller } = install();
    history.replaceState(null, "", "/#/changes");
    controller.sync(lensSnapshot());
    history.pushState(null, "", "/#/changes/change%3Asha256%3Asame");
    const exact = exactSnapshot("change:sha256:same");
    controller.sync(exact);

    const oldDetailControl = document.createElement("button");
    oldDetailControl.textContent = "generation one action";
    document.querySelector("#detail-body")?.append(oldDetailControl);
    oldDetailControl.focus();
    expect(document.activeElement).toBe(oldDetailControl);

    const nextGeneration = document.createElement("p");
    nextGeneration.textContent = "generation two exact detail";
    document.querySelector("#detail-body")?.replaceChildren(nextGeneration);
    controller.sync(exact);
    expect(document.activeElement).toBe(
      document.querySelector("#detail-close"),
    );

    const retainedFocus = document.querySelector<HTMLElement>("#filter-text");
    retainedFocus?.focus();
    controller.sync(exact);
    expect(document.activeElement).toBe(retainedFocus);
  });

  it("falls back to the active surface when a modal opener was detached", () => {
    const { controller } = install();
    history.replaceState(null, "", "/#/changes");
    controller.sync(lensSnapshot());
    const lensOpener = document.createElement("button");
    document.querySelector("#master")?.append(lensOpener);
    lensOpener.focus();
    document.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "?",
        bubbles: true,
        cancelable: true,
      }),
    );
    lensOpener.remove();
    document.querySelector<HTMLButtonElement>("#key-help-close")?.click();
    expect(document.activeElement).toBe(document.querySelector("#master"));

    history.pushState(null, "", "/#/changes/change%3Asha256%3Amodal");
    controller.sync(exactSnapshot("change:sha256:modal"));
    const exactOpener = document.createElement("button");
    document.querySelector("#detail-body")?.append(exactOpener);
    exactOpener.focus();
    document.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "k",
        metaKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    exactOpener.remove();
    document.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(document.activeElement).toBe(
      document.querySelector("#detail-close"),
    );
  });

  it("returns through the exact history entry's originating lens", () => {
    const { controller, navigate } = install();
    history.replaceState(null, "", "/#/attention?q=needs-review");
    controller.sync(attentionSnapshot());
    history.pushState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aorigin?q=needs-review",
    );
    const exact = exactSnapshot("change:sha256:origin", {
      q: "needs-review",
    });
    controller.sync(exact);
    const exactHistoryState = history.state;

    // Visiting another lens must not mutate the return target already recorded
    // on the exact entry. This models Back -> another lens -> Forward.
    history.pushState(null, "", "/#/changes");
    controller.sync(lensSnapshot());
    history.replaceState(
      exactHistoryState,
      "",
      "/#/changes/change%3Asha256%3Aorigin?q=needs-review",
    );
    controller.sync(exact);
    document.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(navigate).toHaveBeenLastCalledWith({
      kind: "lens",
      lens: "attention",
      query: { q: "needs-review" },
    });
  });

  it("uses Timeline for a direct exact link after the prior composition stops", () => {
    const first = install();
    history.replaceState(null, "", "/#/attention?q=needs-review");
    first.controller.sync(attentionSnapshot());
    history.pushState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aprior?q=needs-review",
    );
    first.controller.sync(
      exactSnapshot("change:sha256:prior", { q: "needs-review" }),
    );
    first.controller.stop();
    document.querySelector<HTMLButtonElement>("#detail-close")?.click();
    expect(first.navigate).not.toHaveBeenCalled();

    history.replaceState(null, "", "/#/changes/change%3Asha256%3Adirect");
    const second = install();
    second.controller.sync(exactSnapshot("change:sha256:direct"));
    document.querySelector<HTMLButtonElement>("#detail-close")?.click();

    expect(second.navigate).toHaveBeenLastCalledWith({
      kind: "timeline",
      historyQuery: {},
    });
  });

  it("does not retain a stopped composition callback in module state", async () => {
    const source = await readFile(
      resolve(process.cwd(), "src/change-inspector-interaction.ts"),
      "utf8",
    );

    expect(source).not.toMatch(/let cleanup:\s*\(\(\) => void\)/);
    expect(source).not.toContain("cleanup?.()");
    expect(source).not.toContain("cleanup = stop");
    expect(source).not.toMatch(
      /^let (selectedChangeId|modalReturnFocus|detailReturnFocus|detailWasOpen|currentRoute|exactOriginLens|detailDomIdentity)/m,
    );
    expect(source).not.toContain("__pointbreakChangeRoute");
  });
});
