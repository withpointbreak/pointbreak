import { afterEach, describe, expect, it, vi } from "vitest";
import {
  changeInspectorTimelineGroupAt,
  changeInspectorTimelineNavigableEventIds,
  remeasureChangeInspectorTimelineRows,
  renderChangeInspectorTimeline,
  revealChangeInspectorTimelineEvent,
  setChangeInspectorTimelineGroupExpanded,
} from "../src/change-inspector-timeline";
import type {
  EventHistoryDocument,
  EventHistoryEntry,
  EventHistoryEventType,
} from "../src/change-protocol";
import { ALL_EMITTABLE_CLASSES } from "../src/classNames";
import { authorityCursor } from "./support/authority";
import { mountInspectorDom, resetDom } from "./support/dom";

function documentValue(): EventHistoryDocument {
  return {
    schema: "pointbreak.inspect-event-history",
    version: 1,
    authorityCursor: authorityCursor(3),
    sourceChangeProjectionStamp: "sha256:change",
    timelineProjectionStamp: "sha256:timeline",
    order: "desc",
    eventCount: 3,
    matchCount: 400,
    offset: 100,
    facets: { validation_check_recorded: 1, change_declared: 1 },
    completion: {
      eventTypes: ["validation_check_recorded", "change_declared"],
      trackIds: ["author"],
      changeIds: ["change:sha256:one", "change:sha256:two"],
      revisionRefs: [
        {
          revisionId: "rev:sha256:one",
          objectArtifactContentHash: "sha256:artifact-one",
        },
      ],
      unresolvedRevisionIds: ["rev:sha256:unresolved"],
    },
    diagnostics: ["one legacy event has no exact Revision artifact"],
    queryNotices: ["search was normalized to lowercase"],
    previous: "previous-token",
    next: "next-token",
    entries: [
      {
        eventId: "evt:sha256:one",
        eventType: "validation_check_recorded",
        occurredAt: "2026-08-08T00:00:00Z",
        payloadHash: "sha256:payload-one",
        journalId: "journal:sha256:one",
        trackId: "author",
        writer: {
          actorId: "actor:author",
          producer: { name: "pointbreak", version: "0.10.0" },
        },
        verificationStatus: "valid",
        assertionMode: "advisory",
        subject: {
          kind: "review",
          target: { kind: "revision", revisionId: "rev:sha256:one" },
        },
        changeIds: ["change:sha256:one"],
        revisionRefs: [
          {
            revisionId: "rev:sha256:one",
            objectArtifactContentHash: "sha256:artifact-one",
          },
        ],
        unresolvedRevisionIds: ["rev:sha256:unresolved"],
        sourceRef: {
          sourceSystem: "legacy-review-journal",
          sourceId: "event:legacy:one",
        },
        ingest: {
          via: "ingest-events",
          receivedAt: "2026-08-08T00:00:02Z",
        },
        summary: {
          kind: "validation_check_recorded",
          details: {
            validationCheckId: "validation:sha256:one",
            target: { kind: "revision", revisionId: "rev:sha256:one" },
            checkName: "Web checks",
            command: "npm test",
            status: "passed",
            exitCode: 0,
            trigger: "manual",
            summary: "The focused Inspector checks passed.",
          },
        },
      },
      {
        eventId: "evt:sha256:two",
        eventType: "change_declared",
        occurredAt: "2026-08-08T00:00:01Z",
        payloadHash: "sha256:payload-two",
        journalId: "journal:sha256:one",
        writer: {
          actorId: "actor:migrator",
          producer: { name: "pointbreak", version: "0.10.0" },
        },
        verificationStatus: "unsigned",
        assertionMode: "operative",
        subject: { kind: "change", changeId: "change:sha256:one" },
        changeIds: ["change:sha256:one", "change:sha256:two"],
        revisionRefs: [],
        unresolvedRevisionIds: [],
        summary: {
          kind: "change_declared",
          details: {
            schema: "pointbreak.change-declared",
            version: 1,
            declarationClaimId: "change-declaration:sha256:one",
            changeId: "change:sha256:one",
            identityDescriptor: {
              kind: "root_revision",
              schema: "pointbreak.change-identity.v1",
              revision_id: "rev:sha256:one",
            },
            claimNonce: "nonce-one",
          },
        },
      },
    ],
  };
}

function longDocument(count = 100): EventHistoryDocument {
  const timeline = documentValue();
  // Alternate the two fixture templates so no adjacent same-type run reaches
  // the grouping threshold: these suites exercise virtual geometry over one
  // visual row per event.
  const templates = timeline.entries.slice(0, 2);
  if (templates.length !== 2) throw new Error("missing Timeline entry fixture");
  timeline.eventCount = count;
  timeline.matchCount = count;
  timeline.offset = 0;
  timeline.previous = undefined;
  timeline.next = undefined;
  timeline.entries = Array.from({ length: count }, (_, index) => ({
    ...(templates[index % 2] as EventHistoryEntry),
    eventId: `evt:sha256:${index.toString().padStart(3, "0")}`,
  })) satisfies EventHistoryEntry[];
  return timeline;
}

/** One entry of a supported type, built from the fixture templates. */
function historyEntry(
  eventId: string,
  eventType: Extract<
    EventHistoryEventType,
    | "validation_check_recorded"
    | "change_declared"
    | "review_observation_recorded"
  >,
): EventHistoryEntry {
  const [validation, declared] = documentValue().entries;
  if (!validation || !declared) throw new Error("missing fixture entries");
  if (eventType === "validation_check_recorded") {
    return { ...validation, eventId };
  }
  if (eventType === "change_declared") return { ...declared, eventId };
  return {
    ...validation,
    eventId,
    eventType,
    summary: {
      kind: "review_observation_recorded",
      details: {
        observationId: `obs:sha256:${eventId}`,
        target: { kind: "revision", revisionId: "rev:sha256:one" },
        title: `Observation ${eventId}`,
      },
    },
  };
}

function timelineDocument(entries: EventHistoryEntry[]): EventHistoryDocument {
  const timeline = documentValue();
  timeline.entries = entries;
  timeline.eventCount = entries.length;
  timeline.matchCount = entries.length;
  timeline.offset = 0;
  timeline.previous = undefined;
  timeline.next = undefined;
  return timeline;
}

/** ev:a1, then a run of three validations (ev:b2, ev:c3, ev:d4), then ev:e5. */
function groupedDocument(): EventHistoryDocument {
  return timelineDocument([
    historyEntry("ev:a1", "change_declared"),
    historyEntry("ev:b2", "validation_check_recorded"),
    historyEntry("ev:c3", "validation_check_recorded"),
    historyEntry("ev:d4", "validation_check_recorded"),
    historyEntry("ev:e5", "review_observation_recorded"),
  ]);
}

function renderGroupedTimeline(
  timeline: EventHistoryDocument = groupedDocument(),
  route: { kind: "timeline"; historyQuery: Record<string, string> } = {
    kind: "timeline",
    historyQuery: {},
  },
): HTMLElement {
  mountInspectorDom();
  const master = document.querySelector<HTMLElement>("#master");
  if (!master) throw new Error("missing master");
  renderChangeInspectorTimeline(
    master,
    timeline,
    { navigate: () => undefined },
    route,
  );
  return master;
}

function renderedEventIds(): string[] {
  return Array.from(
    document.querySelectorAll<HTMLElement>("#timeline [data-event-id]"),
  ).map((row) => row.dataset.eventId ?? "");
}

function setViewportHeight(list: HTMLOListElement, height: number): void {
  Object.defineProperty(list, "clientHeight", {
    configurable: true,
    value: height,
  });
}

function rect(top: number, height: number): DOMRect {
  return {
    x: 0,
    y: top,
    width: 800,
    height,
    top,
    right: 800,
    bottom: top + height,
    left: 0,
    toJSON: () => ({}),
  };
}

afterEach(() => {
  vi.restoreAllMocks();
  resetDom();
});

describe("Change-aware Timeline renderer", () => {
  it("keeps a page-local virtual window with pure exact-event options", () => {
    mountInspectorDom();
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    const navigated: unknown[] = [];
    renderChangeInspectorTimeline(
      master,
      documentValue(),
      { navigate: (route) => navigated.push(route) },
      { kind: "timeline", historyQuery: {} },
      "evt:sha256:one",
    );

    const list = document.querySelector<HTMLOListElement>("#timeline");
    expect(list?.querySelectorAll("[data-timeline-spacer]")).toHaveLength(2);
    expect(list?.querySelectorAll("li.event")).toHaveLength(2);
    const heading = master.querySelector("h1.lens-heading");
    expect(master.querySelectorAll("h1")).toHaveLength(1);
    expect(heading?.textContent).toBe("Timeline");
    expect(master.querySelector(".lens-meta")?.textContent).toBe(
      "400 events · newest first",
    );
    expect(master.textContent).toContain("loaded 101-102 of 400 matches");
    expect(master.textContent).toContain(
      "Presentation chronology uses writer timestamps",
    );
    expect(master.textContent).toContain(
      "Query notice: search was normalized to lowercase",
    );
    expect(master.textContent).toContain(
      "Timeline diagnostic: one legacy event has no exact Revision artifact",
    );
    expect(master.textContent).toContain("Web checks: passed");
    expect(master.textContent).toContain("Revisions rev:sha256:one");
    expect(master.textContent).toContain("actor:author");
    expect(master.textContent).toContain("Changes change:sha256:one");
    expect(master.textContent).toContain("unresolved rev:sha256:unresolved");
    expect(master.textContent).not.toContain("pointbreak 0.10.0");
    expect(master.textContent).not.toContain("advisory assertion");
    expect(master.textContent).not.toContain("legacy-review-journal");
    expect(master.textContent).not.toContain("ingested via ingest-events");
    expect(master.textContent).toContain("verify: valid");
    expect(master.textContent).toContain("Previous page");
    expect(master.textContent).toContain("Next page");
    expect(
      document.querySelector('[data-timeline-page="previous"]')?.textContent,
    ).toBe("Previous page");
    expect(
      document.querySelector('[data-timeline-page="next"]')?.textContent,
    ).toBe("Next page");
    expect(document.querySelector("li.event .time")).not.toBeNull();
    expect(document.querySelector("li.event .rail")).not.toBeNull();
    expect(document.querySelector("li.event .body")).not.toBeNull();
    expect(document.querySelector("li.event .title")).not.toBeNull();
    expect(
      Array.from(
        list?.querySelectorAll<HTMLElement>("[data-timeline-spacer]") ?? [],
      ).map((spacer) => spacer.style.height),
    ).toEqual(["0px", "0px"]);
    expect(document.querySelector("li.event")?.getAttribute("role")).toBe(
      "option",
    );
    expect(list?.getAttribute("role")).toBe("listbox");
    expect(list?.dataset.timelineRoute).toBe("#/timeline");
    expect(list?.tabIndex).toBe(0);
    expect(list?.querySelectorAll('[tabindex="0"]')).toHaveLength(0);
    expect(list?.querySelectorAll('li.event[tabindex="-1"]')).toHaveLength(2);
    expect(
      Array.from(
        list?.querySelectorAll<HTMLAnchorElement>("a.ref") ?? [],
      ).every((link) => link.tabIndex === -1),
    ).toBe(true);
    expect(
      document.querySelectorAll('.timeline-shell [tabindex="0"]'),
    ).toHaveLength(1);
    expect(list?.getAttribute("aria-activedescendant")).toBe(
      document.querySelector<HTMLElement>("li.event")?.id,
    );
    expect(
      document.querySelector("li.event")?.getAttribute("aria-selected"),
    ).toBe("true");
    expect(
      document.querySelector<HTMLElement>("li.event .rail")?.style.background,
    ).toContain("--evt-validation");
    expect(document.querySelector("li.event button")).toBeNull();

    document.querySelector<HTMLElement>("li.event")?.click();
    expect(navigated).toEqual([]);
  });

  it("keeps Timeline rows compact while retaining every exact context action", () => {
    mountInspectorDom();
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    const timeline = documentValue();
    const entry = timeline.entries[0];
    if (entry?.summary.kind !== "validation_check_recorded") {
      throw new Error("missing validation entry");
    }
    const eventId =
      "evt:sha256:1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
    const changeA =
      "change:sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const changeB =
      "change:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const revisionA =
      "rev:sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const revisionB =
      "rev:sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    const artifactA =
      "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    const artifactB =
      "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    timeline.entries = [
      {
        ...entry,
        eventId,
        changeIds: [changeA, changeB],
        revisionRefs: [
          { revisionId: revisionA, objectArtifactContentHash: artifactA },
          { revisionId: revisionB, objectArtifactContentHash: artifactB },
        ],
        unresolvedRevisionIds: [revisionB],
        summary: {
          ...entry.summary,
          details: {
            ...entry.summary.details,
            summary: `${"Long reader-facing validation context. ".repeat(12)}${revisionA}`,
          },
        },
      },
    ];
    timeline.eventCount = 1;
    timeline.matchCount = 1;
    timeline.offset = 0;
    timeline.previous = undefined;
    timeline.next = undefined;

    renderChangeInspectorTimeline(
      master,
      timeline,
      { navigate: () => undefined },
      { kind: "timeline", historyQuery: {} },
    );

    const row = document.querySelector<HTMLElement>("li.event");
    if (!row) throw new Error("missing Timeline row");
    expect(row.textContent).toContain("evt:12345678");
    expect(row.textContent).toContain("change:aaaaaaaa");
    expect(row.textContent).toContain("change:bbbbbbbb");
    expect(row.textContent).toContain("rev:cccccccc");
    expect(row.textContent).toContain("rev:dddddddd");
    expect(row.textContent).not.toContain(eventId);
    expect(row.textContent).not.toContain(changeA);
    expect(row.textContent).not.toContain(revisionA);
    expect(row.textContent).not.toContain(artifactA);
    expect(
      row.querySelector(".event-summary")?.textContent?.length,
    ).toBeLessThanOrEqual(180);
    expect(row.querySelector(".event-summary")?.textContent).toContain("…");
    expect(row.getAttribute("aria-label")).toContain(eventId);
    expect(row.getAttribute("aria-label")).toContain(changeA);
    expect(row.getAttribute("aria-label")).toContain(revisionA);

    const contexts = Array.from(
      row.querySelectorAll<HTMLAnchorElement>("a[data-timeline-context-id]"),
    );
    // The row's track and writer are filter links too, painted in the meta
    // block ahead of the event, Change and exact Revision context links.
    expect(contexts.map((link) => link.dataset.timelineContextId)).toEqual([
      "author",
      "actor:author",
      eventId,
      changeA,
      changeB,
      revisionA,
      revisionB,
    ]);
    expect(contexts.map((link) => link.getAttribute("tabindex"))).toEqual([
      "-1",
      "-1",
      "-1",
      "-1",
      "-1",
      "-1",
      "-1",
    ]);
    expect(contexts.map((link) => link.getAttribute("title"))).toEqual([
      "track author",
      "writer actor:author",
      eventId,
      changeA,
      changeB,
      `exact Revision ${revisionA}; artifact ${artifactA}`,
      `exact Revision ${revisionB}; artifact ${artifactB}`,
    ]);
    expect(contexts[3]?.getAttribute("href")).toContain(
      encodeURIComponent(changeA),
    );
    expect(contexts[5]?.getAttribute("href")).toContain(
      `revision=${encodeURIComponent(revisionA)}`,
    );
    expect(contexts[5]?.getAttribute("href")).toContain(
      `artifactHash=${encodeURIComponent(artifactA)}`,
    );
    expect(contexts[5]?.dataset.revisionId).toBe(revisionA);
    expect(contexts[5]?.dataset.artifactHash).toBe(artifactA);
    expect(contexts[5]?.getAttribute("aria-label")).toContain(revisionA);
    expect(contexts[5]?.getAttribute("aria-label")).toContain(artifactA);
    // The two exact Revision filters stand alone: neither silently picks one
    // of the two explicit Change contexts as their owner.
    expect(contexts[5]?.getAttribute("href")).not.toContain("change=");
    expect(contexts[6]?.getAttribute("href")).not.toContain("change=");
  });

  it("filters the Timeline to a row's writer without disturbing the rest of the query", () => {
    mountInspectorDom();
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    const route = {
      kind: "timeline" as const,
      historyQuery: {
        q: "type:observation",
        track: "author",
        order: "asc" as const,
        after: "page-2",
      },
    };
    renderChangeInspectorTimeline(
      master,
      documentValue(),
      { navigate: () => undefined },
      route,
    );
    const actor = document.querySelector<HTMLAnchorElement>(
      'li.event a[data-timeline-context-kind="actor"]',
    );
    expect(actor?.textContent).toBe("actor:author");
    expect(actor?.tabIndex).toBe(-1);
    expect(actor?.getAttribute("aria-label")).toBe(
      "Filter Timeline to writer actor:author",
    );
    // The actor is a q clause; `track` stays the structured param it already
    // was, and the continuation cursor is dropped.
    expect(actor?.getAttribute("href")).toBe(
      "#/timeline?q=type%3Aobservation+actor%3Aauthor&track=author&order=asc",
    );
  });

  it("filters the Timeline to a row's track through the structured param", () => {
    mountInspectorDom();
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    const route = {
      kind: "timeline" as const,
      historyQuery: { q: "type:observation", at: "evt:sha256:one" },
    };
    renderChangeInspectorTimeline(
      master,
      documentValue(),
      { navigate: () => undefined },
      route,
    );
    const track = document.querySelector<HTMLAnchorElement>(
      'li.event a[data-timeline-context-kind="track"]',
    );
    expect(track?.textContent).toBe("track author");
    expect(track?.getAttribute("aria-label")).toBe(
      "Filter Timeline to track author",
    );
    // Never flattened into the query text.
    expect(track?.getAttribute("href")).toBe(
      "#/timeline?q=type%3Aobservation&track=author",
    );
  });

  it("does not re-append an actor clause that already filters", () => {
    mountInspectorDom();
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    renderChangeInspectorTimeline(
      master,
      documentValue(),
      { navigate: () => undefined },
      { kind: "timeline", historyQuery: { q: "actor:author" } },
    );
    expect(
      document
        .querySelector<HTMLAnchorElement>(
          'li.event a[data-timeline-context-kind="actor"]',
        )
        ?.getAttribute("href"),
    ).toBe("#/timeline?q=actor%3Aauthor");
  });

  it("keeps a filter link from also opening the event row", () => {
    mountInspectorDom();
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    renderChangeInspectorTimeline(
      master,
      documentValue(),
      { navigate: () => undefined },
      { kind: "timeline", historyQuery: {} },
    );
    const actor = document.querySelector<HTMLElement>(
      'li.event a[data-timeline-context-kind="actor"]',
    );
    expect(actor?.closest("li.event")).not.toBeNull();
    expect(actor?.matches("a[href]")).toBe(true);
    // An actor click writes q only — no `track` param appears.
    expect(actor?.getAttribute("href")).toBe("#/timeline?q=actor%3Aauthor");
    expect(document.querySelectorAll("li.event button")).toHaveLength(0);
  });

  it("bounds opaque presentation titles without losing their exact source", () => {
    mountInspectorDom();
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    const timeline = documentValue();
    const entry = timeline.entries[0];
    if (!entry) throw new Error("missing Timeline entry");
    const observationId =
      "obs:sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const longTitle = `${observationId} ${"reader-facing detail ".repeat(12)}`;
    timeline.entries = [
      {
        ...entry,
        eventType: "review_observation_recorded",
        summary: {
          kind: "review_observation_recorded",
          details: {
            observationId,
            target: { kind: "revision", revisionId: "rev:sha256:one" },
            title: longTitle,
          },
        },
      },
    ];
    timeline.eventCount = 1;
    timeline.matchCount = 1;
    timeline.offset = 0;
    timeline.previous = undefined;
    timeline.next = undefined;

    renderChangeInspectorTimeline(
      master,
      timeline,
      { navigate: () => undefined },
      { kind: "timeline", historyQuery: {} },
    );

    const row = document.querySelector<HTMLElement>("li.event");
    const heading = row?.querySelector<HTMLHeadingElement>(".title");
    expect(heading?.textContent).toContain("obs:aaaaaaaa");
    expect(heading?.textContent).not.toContain(observationId);
    expect(heading?.textContent?.length).toBeLessThanOrEqual(120);
    expect(heading?.textContent?.endsWith("…")).toBe(true);
    expect(heading?.getAttribute("title")).toBe(longTitle);
    expect(row?.getAttribute("aria-label")).toContain(longTitle);
  });

  it("reveals an exact deep-linked event outside the first virtual window", () => {
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(240);
    mountInspectorDom();
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    const selectedEventId = "evt:sha256:090";

    renderChangeInspectorTimeline(
      master,
      longDocument(),
      { navigate: () => undefined },
      { kind: "timeline", historyQuery: {} },
      selectedEventId,
    );

    const list = document.querySelector<HTMLOListElement>("#timeline");
    if (!list) throw new Error("missing Timeline list");
    const selected = list.querySelector<HTMLElement>(
      `[data-event-id="${selectedEventId}"]`,
    );
    expect(selected).not.toBeNull();
    expect(selected?.getAttribute("aria-selected")).toBe("true");
    expect(list.getAttribute("aria-activedescendant")).toBe(selected?.id);
    expect(list.scrollTop).toBeGreaterThan(0);
    expect(list.querySelectorAll("li.event").length).toBeLessThan(100);
  });

  it("preserves a local exact-event cursor across a same-route repaint", () => {
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(240);
    mountInspectorDom();
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    const timeline = longDocument();
    const route = { kind: "timeline" as const, historyQuery: {} };
    const routedEventId = "evt:sha256:090";
    const localEventId = "evt:sha256:050";

    renderChangeInspectorTimeline(
      master,
      timeline,
      { navigate: () => undefined },
      route,
      routedEventId,
    );
    expect(revealChangeInspectorTimelineEvent(localEventId)).toBe(true);

    // A poll or resize can repaint the same exact-event route. The routed
    // detail remains event 90, but the page-local reading cursor remains on
    // event 50 until the reader explicitly activates another event.
    renderChangeInspectorTimeline(
      master,
      timeline,
      { navigate: () => undefined },
      route,
      routedEventId,
    );

    const list = document.querySelector<HTMLOListElement>("#timeline");
    if (!list) throw new Error("missing Timeline list");
    let localCursor = list.querySelector<HTMLElement>(
      `[data-event-id="${localEventId}"]`,
    );
    expect(localCursor?.getAttribute("aria-selected")).toBe("true");
    expect(list.getAttribute("aria-activedescendant")).toBe(localCursor?.id);

    list.scrollTop = 0;
    list.dispatchEvent(new Event("scroll"));
    expect(list.querySelector(`[data-event-id="${localEventId}"]`)).toBeNull();
    expect(list.getAttribute("aria-activedescendant")).toBeNull();

    list.scrollTop = 50 * 72;
    list.dispatchEvent(new Event("scroll"));
    localCursor = list.querySelector<HTMLElement>(
      `[data-event-id="${localEventId}"]`,
    );
    expect(localCursor?.getAttribute("aria-selected")).toBe("true");
    expect(list.getAttribute("aria-activedescendant")).toBe(localCursor?.id);
  });

  it("snaps a variable-height boundary event into the virtual window", () => {
    mountInspectorDom();
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    renderChangeInspectorTimeline(
      master,
      longDocument(),
      { navigate: () => undefined },
      { kind: "timeline", historyQuery: {} },
    );
    const list = document.querySelector<HTMLOListElement>("#timeline");
    if (!list) throw new Error("missing Timeline list");
    Object.defineProperty(list, "clientHeight", {
      configurable: true,
      value: 240,
    });
    Object.defineProperty(list, "scrollHeight", {
      configurable: true,
      value: 7_200,
    });
    let scrollTop = 0;
    const scrollWrites: number[] = [];
    Object.defineProperty(list, "scrollTop", {
      configurable: true,
      get: () => scrollTop,
      set: (value: number) => {
        scrollWrites.push(value);
        // Model cumulative variable-row error: the mean-height estimate lands
        // short, while the browser's real bottom extent reaches the last row.
        scrollTop = value >= 7_200 ? 6_960 : Math.min(value, 5_000);
      },
    });

    const selectedEventId = "evt:sha256:099";
    expect(revealChangeInspectorTimelineEvent(selectedEventId)).toBe(true);
    const selected = list.querySelector<HTMLElement>(
      `[data-event-id="${selectedEventId}"]`,
    );
    expect(scrollWrites).toEqual([6_960, 7_200]);
    expect(selected?.getAttribute("aria-selected")).toBe("true");
    expect(list.getAttribute("aria-activedescendant")).toBe(selected?.id);
    expect(list.querySelectorAll("li.event").length).toBeLessThan(100);
  });

  it("announces an honest empty result without creating a focusable list", () => {
    mountInspectorDom();
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    const timeline = documentValue();
    timeline.entries = [];
    timeline.matchCount = 0;
    timeline.offset = 0;
    timeline.previous = undefined;
    timeline.next = undefined;

    renderChangeInspectorTimeline(
      master,
      timeline,
      { navigate: () => undefined },
      { kind: "timeline", historyQuery: { q: "no-match" } },
    );

    const list = document.querySelector<HTMLOListElement>("#timeline");
    expect(master.textContent).toContain("loaded 0-0 of 0 matches");
    expect(master.textContent).toContain(
      "No Timeline events match the current filters.",
    );
    expect(list?.tabIndex).toBe(-1);
    expect(list?.getAttribute("aria-disabled")).toBe("true");
    expect(list?.querySelectorAll("li.event")).toHaveLength(0);
  });

  it("drops the at locator when either signed adjacent-page token is followed", () => {
    mountInspectorDom();
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    const navigated: unknown[] = [];
    renderChangeInspectorTimeline(
      master,
      documentValue(),
      { navigate: (route) => navigated.push(route) },
      {
        kind: "timeline",
        historyQuery: {
          at: "evt:sha256:one",
          q: "accepted",
          limit: 25,
        },
      },
    );

    document
      .querySelector<HTMLButtonElement>('[data-timeline-page="previous"]')
      ?.click();
    document
      .querySelector<HTMLButtonElement>('[data-timeline-page="next"]')
      ?.click();
    expect(navigated).toEqual([
      {
        kind: "timeline",
        historyQuery: {
          at: undefined,
          q: "accepted",
          limit: 25,
          after: "previous-token",
        },
      },
      {
        kind: "timeline",
        historyQuery: {
          at: undefined,
          q: "accepted",
          limit: 25,
          after: "next-token",
        },
      },
    ]);
  });

  it("keeps the materialized DOM bounded once the viewport has geometry", () => {
    mountInspectorDom();
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    renderChangeInspectorTimeline(
      master,
      longDocument(),
      { navigate: () => undefined },
      { kind: "timeline", historyQuery: {} },
    );

    const list = document.querySelector<HTMLOListElement>("#timeline");
    if (!list) throw new Error("missing Timeline list");
    setViewportHeight(list, 240);
    list.dispatchEvent(new Event("scroll"));

    expect(list.querySelectorAll("li.event").length).toBeLessThanOrEqual(20);
    expect(list.querySelectorAll("[data-timeline-spacer]")).toHaveLength(2);
    list.scrollTop = 72 * 50;
    list.dispatchEvent(new Event("scroll"));
    expect(list.querySelectorAll("li.event").length).toBeLessThanOrEqual(20);
    expect(
      Array.from(list.querySelectorAll<HTMLElement>("li.event")).some(
        (row) => row.dataset.eventId === "evt:sha256:050",
      ),
    ).toBe(true);
  });

  it("preserves the visible row anchor and list focus when row density changes", () => {
    mountInspectorDom();
    const master = document.querySelector<HTMLElement>("#master");
    if (!master) throw new Error("missing master");
    const selectedEventId = "evt:sha256:010";
    renderChangeInspectorTimeline(
      master,
      longDocument(),
      { navigate: () => undefined },
      { kind: "timeline", historyQuery: {} },
      selectedEventId,
    );

    const list = document.querySelector<HTMLOListElement>("#timeline");
    if (!list) throw new Error("missing Timeline list");
    setViewportHeight(list, 240);
    Object.defineProperty(list, "getBoundingClientRect", {
      configurable: true,
      value: () => rect(100, 240),
    });
    list.scrollTop = 72 * 10;
    list.dispatchEvent(new Event("scroll"));
    for (const row of list.querySelectorAll<HTMLElement>("li.event")) {
      const index = Number(row.dataset.eventId?.split(":").at(-1));
      Object.defineProperty(row, "getBoundingClientRect", {
        configurable: true,
        value: () => rect(90 + (index - 10) * 96, 96),
      });
    }
    list.focus();

    expect(remeasureChangeInspectorTimelineRows()).toBe(true);
    expect(list.scrollTop).toBe(970);
    expect(document.activeElement).toBe(list);
    const selected = Array.from(
      list.querySelectorAll<HTMLElement>("li.event"),
    ).find((row) => row.dataset.eventId === selectedEventId);
    expect(selected).toBeDefined();
    expect(list.getAttribute("aria-activedescendant")).toBe(selected?.id);
  });

  describe("grouped rows", () => {
    it("paints a collapsed run as exactly one virtual row", () => {
      renderGroupedTimeline();

      expect(renderedEventIds()).toEqual(["ev:a1", "ev:b2", "ev:e5"]);
      expect(document.querySelectorAll("#timeline li.event")).toHaveLength(3);
    });

    it("marks the group row with its type, member count, and one visual class", () => {
      renderGroupedTimeline();

      const group = document.querySelector<HTMLElement>(
        '#timeline [data-event-id="ev:b2"]',
      );
      expect(group?.dataset.timelineGroup).toBe("validation_check_recorded");
      expect(group?.dataset.timelineGroupSize).toBe("3");
      expect(group?.classList.contains("event")).toBe(true);
      expect(group?.classList.contains("timeline-group")).toBe(true);
      expect(group?.getAttribute("role")).toBe("option");
      expect(group?.tabIndex).toBe(-1);
      expect(group?.hasAttribute("aria-expanded")).toBe(false);
      expect(group?.querySelector(".type-count")?.textContent).toBe("3");
      expect(group?.querySelector(".type")?.textContent).toBe("validation");
      expect(
        group?.querySelector<HTMLElement>(".rail")?.style.background,
      ).toContain("--evt-validation");
      expect(
        document.querySelector("#timeline [data-timeline-group] a"),
      ).toBeNull();
    });

    it("registers the one new class it adds", () => {
      expect(ALL_EMITTABLE_CLASSES).toContain("timeline-group");
    });

    it("discloses the page-scoped collapse in the lens metadata line", () => {
      const master = renderGroupedTimeline();

      expect(master.querySelector(".lens-meta")?.textContent).toBe(
        "5 events · newest first · adjacent same-type events collapsed",
      );
    });

    it("leaves the metadata line unchanged when nothing collapsed", () => {
      const master = renderGroupedTimeline(
        timelineDocument([
          historyEntry("ev:a1", "change_declared"),
          historyEntry("ev:b2", "validation_check_recorded"),
        ]),
      );

      expect(master.querySelector(".lens-meta")?.textContent).toBe(
        "2 events · newest first",
      );
    });

    it("renders a page filtered to exactly one event type flat", () => {
      // A single-type filter is the reader asking for that whole run, so
      // collapsing it would hide the page behind one row.
      renderGroupedTimeline(groupedDocument(), {
        kind: "timeline",
        historyQuery: { type: "validation_check_recorded" },
      });

      expect(renderedEventIds()).toEqual([
        "ev:a1",
        "ev:b2",
        "ev:c3",
        "ev:d4",
        "ev:e5",
      ]);
    });

    it("keeps the row-height estimator converging with group rows present", () => {
      renderGroupedTimeline();
      const list = document.querySelector<HTMLOListElement>("#timeline");
      if (!list) throw new Error("missing Timeline list");
      for (const row of list.querySelectorAll<HTMLElement>("li.event")) {
        // A group row is close to, not exactly, one event row tall.
        const height = row.dataset.timelineGroup ? 60 : 80;
        Object.defineProperty(row, "getBoundingClientRect", {
          configurable: true,
          value: () => rect(0, height),
        });
      }

      expect(remeasureChangeInspectorTimelineRows()).toBe(true);
      // The estimator is a running mean over painted rows; it must move by a
      // bounded amount rather than diverge.
      const spacers = Array.from(
        list.querySelectorAll<HTMLElement>("[data-timeline-spacer]"),
      );
      expect(spacers.map((spacer) => spacer.style.height)).toEqual([
        "0px",
        "0px",
      ]);
    });

    it("exposes the visible sequence and group ownership through one seam", () => {
      renderGroupedTimeline();

      expect(changeInspectorTimelineNavigableEventIds()).toEqual([
        "ev:a1",
        "ev:b2",
        "ev:e5",
      ]);
      expect(changeInspectorTimelineGroupAt("ev:b2")).toBe("ev:b2");
      expect(changeInspectorTimelineGroupAt("ev:c3")).toBe("ev:b2");
      expect(changeInspectorTimelineGroupAt("ev:a1")).toBeNull();
      expect(changeInspectorTimelineGroupAt(null)).toBeNull();

      setChangeInspectorTimelineGroupExpanded("ev:b2", true);

      expect(renderedEventIds()).toEqual([
        "ev:a1",
        "ev:b2",
        "ev:c3",
        "ev:d4",
        "ev:e5",
      ]);
      expect(changeInspectorTimelineNavigableEventIds()).toEqual([
        "ev:a1",
        "ev:b2",
        "ev:c3",
        "ev:d4",
        "ev:e5",
      ]);
      expect(changeInspectorTimelineGroupAt("ev:c3")).toBeNull();
      expect(
        changeInspectorTimelineGroupAt("ev:c3", { includeExpanded: true }),
      ).toBe("ev:b2");

      setChangeInspectorTimelineGroupExpanded("ev:b2", false);

      expect(renderedEventIds()).toEqual(["ev:a1", "ev:b2", "ev:e5"]);
    });

    it("reports raw entry order for a document the renderer has not painted", () => {
      const other = groupedDocument();
      other.timelineProjectionStamp = "sha256:elsewhere";

      expect(changeInspectorTimelineNavigableEventIds(other)).toEqual([
        "ev:a1",
        "ev:b2",
        "ev:c3",
        "ev:d4",
        "ev:e5",
      ]);
    });
  });

  describe("group accessibility", () => {
    it("names a collapsed group by its count, type label, and state", () => {
      // aria-expanded is NOT supported on role=option (WAI-ARIA 1.2), so the
      // state lives in the accessible NAME.
      renderGroupedTimeline();

      const group = document.querySelector<HTMLElement>(
        '#timeline [data-event-id="ev:b2"]',
      );
      expect(group?.getAttribute("role")).toBe("option");
      expect(group?.getAttribute("aria-label")).toBe(
        "Validations, 3 events, collapsed",
      );
      expect(group?.hasAttribute("aria-expanded")).toBe(false);
      expect(group?.querySelector(".title")?.textContent).toBe("Validations");
    });

    it("replaces the group row with a labelled role=group on expand", () => {
      // Expanding REPLACES the group row with its member rows; the container
      // carries the group's identity and there is no surviving controller row.
      renderGroupedTimeline();
      setChangeInspectorTimelineGroupExpanded("ev:b2", true);

      expect(
        document.querySelector('[data-event-id="ev:b2"][role="option"]'),
      ).not.toBeNull();
      expect(
        document.querySelector("#timeline [data-timeline-group]"),
      ).toBeNull();
      const group = document.querySelector("#timeline [role='group']");
      expect(group?.getAttribute("aria-label")).toBe("Validations, 3 events");
      expect(group?.querySelectorAll('[role="option"]').length).toBe(3);
      expect(
        Array.from(
          group?.querySelectorAll<HTMLElement>("[data-event-id]") ?? [],
        ).map((row) => row.dataset.eventId),
      ).toEqual(["ev:b2", "ev:c3", "ev:d4"]);
    });

    it("keeps members individually labelled and free of aria-expanded", () => {
      renderGroupedTimeline();
      setChangeInspectorTimelineGroupExpanded("ev:b2", true);

      const member = document.querySelector('[data-event-id="ev:c3"]');
      expect(member?.getAttribute("aria-label")).toMatch(/validation/);
      expect(member?.getAttribute("aria-label")).toContain("ev:c3");
      expect(member?.hasAttribute("aria-expanded")).toBe(false);
    });

    it("uses no unsupported or interactive constructs inside the listbox", () => {
      renderGroupedTimeline();
      setChangeInspectorTimelineGroupExpanded("ev:b2", true);

      // role=group IS permitted inside a listbox; details/summary and a
      // tabbable button are not, and no option carries aria-expanded.
      expect(document.querySelector("#timeline details")).toBeNull();
      expect(document.querySelector("#timeline button")).toBeNull();
      expect(document.querySelector("#timeline [aria-expanded]")).toBeNull();
      expect(
        document.querySelectorAll('#timeline [tabindex="0"]'),
      ).toHaveLength(0);
    });

    it("keeps a multi-word type label verbatim rather than inventing a title", () => {
      renderGroupedTimeline(
        timelineDocument([
          historyEntry("ev:a1", "change_declared"),
          historyEntry("ev:b2", "change_declared"),
          historyEntry("ev:c3", "change_declared"),
        ]),
      );

      const group = document.querySelector<HTMLElement>(
        '#timeline [data-event-id="ev:a1"]',
      );
      expect(group?.getAttribute("aria-label")).toBe(
        "Change declared, 3 events, collapsed",
      );
      expect(group?.querySelector(".title")?.textContent).toBe(
        "Change declared",
      );
    });
  });
});
