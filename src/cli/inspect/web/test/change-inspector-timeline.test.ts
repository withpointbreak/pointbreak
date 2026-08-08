import { afterEach, describe, expect, it } from "vitest";
import {
  remeasureChangeInspectorTimelineRows,
  renderChangeInspectorTimeline,
} from "../src/change-inspector-timeline";
import type {
  EventHistoryDocument,
  EventHistoryEntry,
} from "../src/change-protocol";
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
  const template = timeline.entries[0];
  if (!template) throw new Error("missing Timeline entry fixture");
  timeline.eventCount = count;
  timeline.matchCount = count;
  timeline.offset = 0;
  timeline.previous = undefined;
  timeline.next = undefined;
  timeline.entries = Array.from({ length: count }, (_, index) => ({
    ...template,
    eventId: `evt:sha256:${index.toString().padStart(3, "0")}`,
  })) satisfies EventHistoryEntry[];
  return timeline;
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

afterEach(resetDom);

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
    expect(master.textContent).toContain("Timeline · 400");
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
    expect(master.textContent).toContain("Revision rev:sha256:one");
    expect(master.textContent).toContain("actor:author · pointbreak 0.10.0");
    expect(master.textContent).toContain("advisory assertion");
    expect(master.textContent).toContain(
      "source legacy-review-journal · event:legacy:one",
    );
    expect(master.textContent).toContain("ingested via ingest-events");
    expect(master.textContent).toContain(
      "exact Revisions: rev:sha256:one · sha256:artifact-one",
    );
    expect(master.textContent).toContain(
      "unresolved Revisions: rev:sha256:unresolved",
    );
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
    expect(list?.tabIndex).toBe(0);
    expect(list?.querySelectorAll('[tabindex="0"]')).toHaveLength(0);
    expect(list?.querySelectorAll('[tabindex="-1"]')).toHaveLength(2);
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
});
