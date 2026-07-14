import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { parseSearchQueryFor } from "../src/query";
import type {
  AttentionDoc,
  AttentionItem,
  HistoryDoc,
  RevisionsDoc,
  ThreadsDoc,
} from "../src/store";
import attentionJson from "./fixtures/attention.json";
import historyJson from "./fixtures/history.json";
import revisionsJson from "./fixtures/revisions.json";
import threadsJson from "./fixtures/threads.json";
import { mountInspectorDom, resetDom } from "./support/dom";
import {
  installFetchMock,
  resetSnapshotResponse,
  uninstallFetchMock,
} from "./support/fetch";

// `render.ts` is the single store-subscriber: a `() => void` that paints one frame
// from `getState()` — stats, diagnostics, the type toggles (facet counts), the lens
// switcher, the master pane (delegating to the lenses), the detail pane (delegating
// to detail), scroll-into-view, and the diff overlay reconciler. It never calls
// `navigate`/`commit`; the once-installed `#master`/`#filter-types` delegates own the
// commits. The store, the render module's `lastMasterLens`, detail's
// `shownCompositeId`, and the overlay manager are module singletons, so reset and
// re-import them before each test.
type Store = typeof import("../src/store");
type Render = typeof import("../src/render");
let store: Store;
let render: Render;

const OBS_EVENT =
  "evt:sha256:8ac34bc85b48ed6623660a174b024bd9099edd09877180bfa87101cc76ac6058";
const REV =
  "rev:sha256:9a7626ca7cb2801721ed992402184460210477aadfd4f7228628b65ff11a6efd";
const OBJ =
  "obj:sha256:38a493d2f09d6fde9d1dcac61a12c4ccc4de42a0b9c6829752d34cc648a9f9d7";

function $<T extends Element = Element>(sel: string): T | null {
  return document.querySelector<T>(sel);
}

beforeEach(async () => {
  const vitest = await import("vitest");
  vitest.vi.resetModules();
  store = await import("../src/store");
  render = await import("../src/render");
  mountInspectorDom();
  installFetchMock();
  history.replaceState(null, "", "/");
  store.commit({
    history: historyJson as unknown as HistoryDoc,
    revisions: revisionsJson as unknown as RevisionsDoc,
    threads: threadsJson as unknown as ThreadsDoc,
  });
  render.initControls();
});

afterEach(() => {
  uninstallFetchMock();
  resetSnapshotResponse();
  resetDom();
});

describe("render is a no-arg projection of getState()", () => {
  it("takes no arguments (the subscribe(render) signature)", () => {
    expect(render.render.length).toBe(0);
  });

  it("paints the stat row from the loaded document counts", () => {
    render.render();
    expect($("#stat-events")?.textContent).toBe("8 events");
    expect($("#stat-units")?.textContent).toBe("1 units");
    expect($("#stat-threads")?.textContent).toBe("1 threads");
    // The freshness hash is the short form of the event-set hash.
    expect($("#stat-hash")?.textContent).toBe("e81f297a301a");
  });

  it("hides diagnostics when empty and surfaces them when present", () => {
    render.render();
    expect($("#diagnostics")?.classList.contains("hidden")).toBe(true);

    store.commit({
      history: {
        ...(historyJson as unknown as HistoryDoc),
        diagnostics: [{ code: "stale-store", message: "reload to refresh" }],
      },
    });
    render.render();
    const diag = $("#diagnostics");
    expect(diag?.classList.contains("hidden")).toBe(false);
    expect(diag?.textContent).toContain("stale-store");
    expect(diag?.textContent).toContain("reload to refresh");
  });
});

describe("the type facet menu (facet distribution + aria-pressed, moved off the toolbar)", () => {
  it("renders one row per present type, inside the popover, with its facet count and pressed state", () => {
    render.render();
    const menu = $("#filter-types-menu");
    expect((menu?.querySelectorAll(".type-facet-row").length ?? 0) > 0).toBe(
      true,
    );
    const obs = $<HTMLElement>('[data-type="review_observation_recorded"]');
    expect(obs).not.toBeNull();
    expect(obs?.closest("#filter-types-menu")).not.toBeNull();
    expect(obs?.getAttribute("aria-pressed")).toBe("true");
    expect(obs?.querySelector(".type-count")?.textContent).toBe("1");
    const assess = $<HTMLElement>('[data-type="review_assessment_recorded"]');
    expect(assess?.querySelector(".type-count")?.textContent).toBe("2");
  });

  it("reads the per-type counts from the server facets, not a client recount", () => {
    // Distinct facet numbers the client could not have derived from the loaded
    // entries prove the rows read the server-computed distribution.
    store.commit({
      history: {
        ...(historyJson as unknown as HistoryDoc),
        facets: {
          review_observation_recorded: 7,
          review_assessment_recorded: 3,
        },
      },
    });
    render.render();
    expect(
      $('[data-type="review_observation_recorded"] .type-count')?.textContent,
    ).toBe("7");
    expect(
      $('[data-type="review_assessment_recorded"] .type-count')?.textContent,
    ).toBe("3");
  });

  it("toggling a type via the menu produces the same ?type= navigation as today (regression pin)", () => {
    render.render();
    const obs = $<HTMLElement>('[data-type="review_observation_recorded"]');
    expect(
      store.getState().enabledTypes.has("review_observation_recorded"),
    ).toBe(true);
    obs?.dispatchEvent(new Event("click", { bubbles: true }));
    expect(
      store.getState().enabledTypes.has("review_observation_recorded"),
    ).toBe(false);
  });

  it("the pills markup is gone — no bare .type-toggle rows sit directly in #filter-types", () => {
    render.render();
    expect(document.querySelectorAll(".type-toggle").length).toBe(0);
    expect(
      document.querySelectorAll("#filter-types > .type-facet-row").length,
    ).toBe(0); // rows live inside the popover, not the container itself
  });

  it("the toggle button shows a count badge reflecting the enabled/present split", () => {
    render.render();
    const btn = $("#filter-types-toggle");
    expect(btn?.textContent).toMatch(/types/i);
    // Every present type is enabled by default (render.ts's default-seeding), so
    // the label shows the total with no fraction.
    expect(btn?.textContent).not.toMatch(/\d+\/\d+/);
  });

  it("shows the enabled/present fraction once a type is toggled off", () => {
    render.render();
    $<HTMLElement>('[data-type="review_observation_recorded"]')?.dispatchEvent(
      new Event("click", { bubbles: true }),
    );
    render.render();
    expect($("#filter-types-toggle")?.textContent).toMatch(/\d+\/\d+/);
  });

  it("enables a type when it first appears without reviving a type the reader hid", () => {
    render.render();
    const initiallyPresent = Object.keys(
      store.getState().history?.facets ?? {},
    );
    store.commit({ enabledTypes: new Set(initiallyPresent) });
    const hidden = initiallyPresent[0];
    store.getState().enabledTypes.delete(hidden);

    const arriving = "review_validation_recorded";
    expect(store.getState().seenTypes.has(arriving)).toBe(false);
    store.commit({
      history: {
        ...(store.getState().history as HistoryDoc),
        facets: {
          ...store.getState().history?.facets,
          [arriving]: 1,
        },
      },
    });
    render.render();

    expect(store.getState().enabledTypes.has(arriving)).toBe(true);
    expect(store.getState().enabledTypes.has(hidden)).toBe(false);
  });

  it("opens the popover on click, closes on outside click without committing anything", () => {
    render.render();
    const toggle = $<HTMLElement>("#filter-types-toggle");
    const menu = $("#filter-types-menu");
    toggle?.dispatchEvent(new Event("click", { bubbles: true }));
    expect(menu?.classList.contains("hidden")).toBe(false);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    document.body.dispatchEvent(new Event("click", { bubbles: true }));
    expect(menu?.classList.contains("hidden")).toBe(true);
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    expect(store.getState().enabledTypes.size).toBeGreaterThan(0); // unchanged, nothing committed
  });

  it("keeps an open popover open across a repaint (a facet refresh never slams it shut)", () => {
    render.render();
    $<HTMLElement>("#filter-types-toggle")?.dispatchEvent(
      new Event("click", { bubbles: true }),
    );
    render.render();
    expect($("#filter-types-menu")?.classList.contains("hidden")).toBe(false);
  });

  it("closes on Escape without committing anything or leaking the key to the global handler", () => {
    render.render();
    $<HTMLElement>("#filter-types-toggle")?.dispatchEvent(
      new Event("click", { bubbles: true }),
    );
    const before = store.getState().filterText;
    const ev = new KeyboardEvent("keydown", { key: "Escape", bubbles: true });
    $("#filter-types")?.dispatchEvent(ev);
    expect($("#filter-types-menu")?.classList.contains("hidden")).toBe(true);
    expect(store.getState().filterText).toBe(before); // the global Escape ladder never ran
  });

  it("Escape returns focus to the toggle (never strands it inside the hidden menu)", () => {
    render.render();
    $<HTMLElement>("#filter-types-toggle")?.dispatchEvent(
      new Event("click", { bubbles: true }),
    );
    const row = $<HTMLElement>(
      "#filter-types-menu .type-facet-row",
    ) as HTMLElement;
    row.focus();
    row.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
    expect($("#filter-types-menu")?.classList.contains("hidden")).toBe(true);
    expect(document.activeElement).toBe($("#filter-types-toggle"));
  });

  it("is Timeline-lens-only visible, and a lens switch forces an open popover shut", () => {
    store.commit({ lens: "timeline" });
    render.render();
    $<HTMLElement>("#filter-types-toggle")?.dispatchEvent(
      new Event("click", { bubbles: true }),
    );
    store.commit({ lens: "list" });
    render.render();
    expect($("#filter-types")?.classList.contains("hidden")).toBe(true);
    store.commit({ lens: "timeline" });
    render.render();
    expect($("#filter-types")?.classList.contains("hidden")).toBe(false);
    // Coming back, the popover is shut — the switch closed it, not just hid it.
    expect($("#filter-types-menu")?.classList.contains("hidden")).toBe(true);
  });
});

describe("applied-filter chips (pure view of filterText)", () => {
  it("renders one chip per parsed qualifier clause and none for free text", () => {
    store.commit({ filterText: "type:observation pinned track:codex" });
    render.render();
    const chips = document.querySelectorAll("#filter-chips .filter-chip");
    expect(chips.length).toBe(2);
    expect(document.querySelector("#filter-chips")?.textContent).not.toContain(
      "pinned",
    );
  });

  it("a chip's ✕ removes exactly that clause and preserves the rest, incl. a duplicate key", () => {
    store.commit({ filterText: "tag:a pinned tag:b" });
    render.render();
    const removeButtons = document.querySelectorAll<HTMLElement>(
      "#filter-chips .filter-chip-remove",
    );
    expect(removeButtons.length).toBe(2);
    removeButtons[1]?.dispatchEvent(new Event("click", { bubbles: true }));
    expect(store.getState().filterText).toBe("tag:a pinned");
  });

  it("chips are derived from filterText on every render, never stored separately", () => {
    store.commit({ filterText: "track:codex" });
    render.render();
    expect(document.querySelectorAll("#filter-chips .filter-chip").length).toBe(
      1,
    );
    store.commit({ filterText: "" });
    render.render();
    expect(document.querySelectorAll("#filter-chips .filter-chip").length).toBe(
      0,
    );
  });

  it("labels an actor chip with the short spelling, not the doubled scheme prefix", () => {
    // The parser canonicalizes actor values to the stored full id
    // (actor:agent:codex); rendering that verbatim after the `actor:` key would
    // read `actor:actor:agent:codex`. The chip label uses the short spelling —
    // the same form the actor-ref click mints into filterText.
    store.commit({ filterText: "actor:agent:codex" });
    render.render();
    const chip = document.querySelector("#filter-chips .filter-chip");
    expect(chip?.textContent).toContain("actor:agent:codex");
    expect(chip?.textContent).not.toContain("actor:actor:");
  });

  it("marks a negated clause's chip with the negated modifier class", () => {
    store.commit({ filterText: "-check:failed" });
    render.render();
    const chip = document.querySelector("#filter-chips .filter-chip");
    expect(chip?.classList.contains("filter-chip-negated")).toBe(true);
  });

  it("parses against the revision surface on the list lens (no chip for type:)", () => {
    store.commit({ lens: "list", filterText: "type:observation actor:codex" });
    render.render();
    const chips = document.querySelectorAll("#filter-chips .filter-chip");
    expect(chips.length).toBe(1);
    expect(chips[0]?.textContent).toContain("actor");
  });
});

describe("renderLensSwitcher + renderMaster (lens dispatch + scaffold)", () => {
  it("groups the lens tabs into record + attention families", () => {
    // Structure pin over the dom.ts mirror (which changes in lockstep with
    // assets/index.html): the Record pair sits in one group, Attention apart,
    // and no umbrella label names the group. The visual grouping itself is
    // verified by eye in the dogfood pass.
    const record = document.querySelector(".lens-group-record");
    expect(record?.querySelectorAll(".lens-tab").length).toBe(2); // timeline, list
    const attention = $('.lens-tab[data-lens="attention"]');
    expect(attention).not.toBeNull();
    expect(attention?.closest(".lens-group-record")).toBeNull(); // set apart
    expect(document.querySelector(".lens-group-label")).toBeNull();
  });

  it("marks the active lens tab and paints the timeline lens by default", () => {
    render.render();
    expect(
      $('.lens-tab[data-lens="timeline"]')?.getAttribute("aria-pressed"),
    ).toBe("true");
    expect($('.lens-tab[data-lens="list"]')?.getAttribute("aria-pressed")).toBe(
      "false",
    );
    const master = $("#master");
    const timeline = master?.querySelector<HTMLElement>("#timeline");
    expect(timeline).not.toBeNull();
    expect(timeline?.getAttribute("aria-label")).toBe("event timeline");
    expect(timeline?.getAttribute("tabindex")).toBe("0");
    expect((timeline?.querySelectorAll(".event").length ?? 0) > 0).toBe(true);
  });

  it("dispatches the list lens to renderRevisionList (#units)", () => {
    store.commit({ lens: "list" });
    render.render();
    expect($('.lens-tab[data-lens="list"]')?.getAttribute("aria-pressed")).toBe(
      "true",
    );
    const master = $("#master");
    expect(master?.querySelector("#units")).not.toBeNull();
    expect(master?.querySelector("#units .unit-card")).not.toBeNull();
  });

  it("dispatches the attention lens to renderAttention (#attention)", () => {
    store.commit({ lens: "attention" });
    render.render();
    const master = $("#master");
    expect(master?.querySelector("#attention")).not.toBeNull();
  });

  it("rebuilds the lens scaffold only on a lens change (idempotent re-render)", () => {
    render.render();
    render.render();
    const master = $("#master");
    // Two renders at the same lens leave exactly one timeline body, repopulated.
    expect(master?.querySelectorAll("#timeline").length).toBe(1);
    expect((master?.querySelectorAll("#timeline .event").length ?? 0) > 0).toBe(
      true,
    );
  });
});

describe("density field tiers", () => {
  it("marks exactly the medium-tier timeline fields for compact hiding", () => {
    const withTags = structuredClone(historyJson) as unknown as HistoryDoc;
    withTags.entries = withTags.entries.map((entry) =>
      entry.eventId === OBS_EVENT
        ? {
            ...entry,
            summary: { ...entry.summary, tags: ["issue:463", "density"] },
          }
        : entry,
    );
    store.commit({ lens: "timeline", history: withTags });
    render.render();

    const row = document.querySelector<HTMLElement>(
      `#timeline li.event[data-event-id="${OBS_EVENT}"]`,
    );
    const medium = row?.querySelectorAll(".tier-medium") ?? [];
    const badges = row?.querySelectorAll(".title .badge") ?? [];
    expect(badges).toHaveLength(2);
    for (const badge of badges)
      expect(badge.classList).toContain("tier-medium");

    const metaItems = Array.from(
      row?.querySelectorAll<HTMLElement>(".meta > span") ?? [],
    );
    const revision = metaItems.find((item) =>
      item.textContent?.startsWith("revision "),
    );
    const anchor = metaItems.find((item) =>
      item.textContent?.startsWith("src/lib.rs:"),
    );
    expect(revision?.classList).toContain("tier-medium");
    expect(anchor?.classList).toContain("tier-medium");
    expect(row?.querySelector(".meta > .tier-medium .verify")).not.toBeNull();
    expect(medium).toHaveLength(5);

    for (const selector of [".time", ".rail", ".title", ".type"]) {
      expect(row?.querySelector(`${selector}.tier-medium`)).toBeNull();
    }
    const track = metaItems.find((item) => item.textContent === "agent:codex");
    expect(track?.closest(".tier-medium")).toBeNull();
    expect(
      row?.querySelector(".ref-actor")?.closest(".tier-medium"),
    ).toBeNull();
  });

  it("hides the revision card kv block at compact but never the attention-cue set", () => {
    store.commit({ lens: "list" });
    render.render();

    const card = document.querySelector("#units .unit-card");
    expect(card?.querySelector(":scope > .kv")?.classList).toContain(
      "tier-medium",
    );
    for (const selector of [
      ":scope > h3",
      ":scope > .supersession-badges",
      ":scope > .overview-summary",
      ":scope > .actions",
    ]) {
      expect(card?.querySelector(selector)?.classList).not.toContain(
        "tier-medium",
      );
    }
    expect(card?.querySelector(".overview-summary .tier-medium")).toBeNull();
  });

  it("keeps mode and the judge-the-ask extras on attention cards at compact", () => {
    store.commit({
      lens: "attention",
      attention: attentionJson as unknown as AttentionDoc,
    });
    render.render();

    const openRequest = document.querySelector(
      '.attention-card[data-entry-id^="open_input_request:"]',
    );
    const cells = Array.from(
      openRequest?.querySelectorAll<HTMLElement>(":scope > .kv > *") ?? [],
    );
    const pair = (key: string): HTMLElement[] => {
      const index = cells.findIndex((cell) => cell.textContent === key);
      return index < 0 ? [] : cells.slice(index, index + 2);
    };
    for (const key of ["reason", "track", "actor", "observed"]) {
      expect(pair(key)).toHaveLength(2);
      for (const cell of pair(key))
        expect(cell.classList).toContain("tier-medium");
    }
    for (const key of ["subject", "mode"]) {
      expect(pair(key)).toHaveLength(2);
      for (const cell of pair(key))
        expect(cell.classList).not.toContain("tier-medium");
    }

    for (const key of ["heads", "assessments", "exit"]) {
      const label = Array.from(
        document.querySelectorAll<HTMLElement>(".attention-card .kv > span"),
      ).find((cell) => cell.textContent === key);
      expect(label).not.toBeUndefined();
      expect(label?.classList).not.toContain("tier-medium");
      expect(label?.nextElementSibling?.classList).not.toContain("tier-medium");
    }
    expect(openRequest?.querySelector("h3 .tier-medium")).toBeNull();
    expect(
      document.querySelector(".attention-freshness.tier-medium"),
    ).toBeNull();
  });
});

describe("toolbar controls are gated per lens (each shows only where its state is consumed)", () => {
  it("shows start and end on every lens, with follow only on the descending timeline", () => {
    for (const lens of ["timeline", "list", "attention"]) {
      store.commit({ lens, order: "desc" });
      render.render();
      expect($("#jump-start")?.classList.contains("hidden"), lens).toBe(false);
      expect($("#jump-end")?.classList.contains("hidden"), lens).toBe(false);
      expect($("#follow-toggle")?.classList.contains("hidden"), lens).toBe(
        lens !== "timeline",
      );
    }

    store.commit({ lens: "timeline", order: "asc" });
    render.render();
    expect($("#follow-toggle")?.classList.contains("hidden")).toBe(true);
  });

  it("shows new-event catch-up at the top of a followed parked timeline", () => {
    store.commit({
      lens: "timeline",
      order: "desc",
      followByLens: { timeline: true, list: false, attention: false },
      timelineHeadAnchor: {
        occurredAt: "2026-07-13T20:00:00Z",
        eventId: "evt:sha256:head",
      },
      timelineNewCount: 3,
    });
    render.render();
    const pill = $("#timeline-new-pill");
    expect(pill?.classList.contains("hidden")).toBe(false);
    expect(pill?.textContent).toBe("Show 3 new events");
    expect(pill?.parentElement?.classList.contains("timeline-shell")).toBe(
      true,
    );
    expect($("header #timeline-new-pill")).toBeNull();

    for (const patch of [
      { followByLens: { timeline: false, list: false, attention: false } },
      {
        followByLens: { timeline: true, list: false, attention: false },
        timelineNewCount: 0,
      },
      { lens: "list", timelineNewCount: 3 },
      { lens: "timeline", order: "asc", timelineNewCount: 3 },
    ]) {
      store.commit(patch);
      render.render();
      const current = $("#timeline-new-pill");
      expect(current == null || current.classList.contains("hidden")).toBe(
        true,
      );
    }
  });

  it("shows the toolbar on both Timeline and Revisions, hides it on Attention", () => {
    store.commit({ lens: "timeline" });
    render.render();
    expect($("#toolbar")?.classList.contains("hidden")).toBe(false);
    store.commit({ lens: "list" });
    render.render();
    expect($("#toolbar")?.classList.contains("hidden")).toBe(false);
    store.commit({ lens: "attention" });
    render.render();
    expect($("#toolbar")?.classList.contains("hidden")).toBe(true);
  });

  it("hides the type page-set control on the Revisions lens (inert there — a click would silently mutate Timeline's ?type=)", () => {
    store.commit({ lens: "timeline" });
    render.render();
    expect($("#filter-types")?.classList.contains("hidden")).toBe(false);
    store.commit({ lens: "list" });
    render.render();
    expect($("#filter-types")?.classList.contains("hidden")).toBe(true);
  });

  it("shows the sort picker only on the list lens, reflecting state.sortKey", () => {
    store.commit({ lens: "timeline" });
    render.render();
    expect($("#sort-picker")?.classList.contains("hidden")).toBe(true);
    expect($("#sort-label")?.classList.contains("hidden")).toBe(true);
    store.commit({ lens: "list", sortKey: "activity" });
    render.render();
    expect($("#sort-picker")?.classList.contains("hidden")).toBe(false);
    expect($("#sort-label")?.classList.contains("hidden")).toBe(false);
    expect($<HTMLSelectElement>("#sort-picker")?.value).toBe("activity");
  });

  it("relabels the order-toggle title per lens and keeps a directional glyph", () => {
    store.commit({ lens: "timeline", order: "desc" });
    render.render();
    expect($("#order-toggle")?.getAttribute("title")).toBe(
      "toggle timeline order",
    );
    expect($("#order-toggle")?.textContent).toBe("↓ newest first");
    store.commit({ lens: "list" });
    render.render();
    expect($("#order-toggle")?.getAttribute("title")).toBe(
      "toggle revision order",
    );
    store.commit({ order: "asc" });
    render.render();
    expect($("#order-toggle")?.textContent).toBe("↑ oldest first");
  });

  it("labels the Attention lens with its fixed order, and offers no sort control", () => {
    store.commit({
      lens: "attention",
      attention: {
        items: [{ id: "a", kind: "open_input_request", tier: "primary" }],
        eventSetHash: "sha256:order-label-test",
      } as AttentionDoc,
    });
    render.render();
    expect($("#attention")?.textContent).toContain("longest waiting first");
    expect($("#sort-picker")).not.toBeNull(); // exists in the DOM, just hidden
    expect($("#sort-picker")?.classList.contains("hidden")).toBe(true);
  });
});

describe("the attention tab count badge (global judgment-queue counts)", () => {
  // The badge is a pure projection of the already-loaded /api/attention items:
  // the needs-input count as the number, the advisory count muted beside it.
  // It reflects the store-wide queue from every lens; the per-revision view is
  // the detail pane's job.
  const item = (id: string, tier: string): AttentionItem => ({
    id,
    kind: "open_input_request",
    tier,
  });
  const attentionOf = (items: AttentionItem[]): AttentionDoc => ({
    items,
    eventSetHash: "sha256:badge-test",
  });

  it("shows the needs-input count on the attention tab", () => {
    store.commit({
      attention: attentionOf([
        item("a", "primary"),
        item("b", "primary"),
        item("c", "secondary"),
      ]),
    });
    render.render();
    const badge = $('.lens-tab[data-lens="attention"] .attention-badge');
    expect(badge?.textContent).toContain("2");
    // Read-only chrome: the tab's own click is the only behavior — the badge
    // carries no control of its own (no dismissal affordance of any kind).
    expect(badge?.querySelector("button, input, a")).toBeNull();
  });

  it("renders the advisory count muted, separately from the primary number", () => {
    store.commit({
      attention: attentionOf([item("a", "primary"), item("c", "secondary")]),
    });
    render.render();
    const secondary = $(
      '.lens-tab[data-lens="attention"] .attention-badge-secondary',
    );
    expect(secondary?.textContent).toContain("1");
  });

  it("keeps the badge for an advisory-only queue (zero of both is the only empty state)", () => {
    store.commit({ attention: attentionOf([item("c", "secondary")]) });
    render.render();
    expect($(".attention-badge")).not.toBeNull();
    expect($(".attention-badge-secondary")?.textContent).toContain("1");
  });

  it("renders no badge element before the attention doc loads or when it is empty", () => {
    render.render(); // attention: null (never committed)
    expect($(".attention-badge")).toBeNull();
    store.commit({ attention: attentionOf([]) });
    render.render();
    expect($(".attention-badge")).toBeNull();
    expect($(".attention-badge-secondary")).toBeNull();
  });

  it("drops the badge on the same repaint that clears the items", () => {
    store.commit({ attention: attentionOf([item("a", "primary")]) });
    render.render();
    expect($(".attention-badge")).not.toBeNull();
    // The derived count fell on its own (a judgment landed elsewhere); the next
    // poll repaint drops the element — no reader action, no lingering zero.
    store.commit({ attention: attentionOf([]) });
    render.render();
    expect($(".attention-badge")).toBeNull();
  });

  it("names both counts accessibly (the badge is not color-only)", () => {
    store.commit({
      attention: attentionOf([item("a", "primary"), item("b", "secondary")]),
    });
    render.render();
    const badge = $('.lens-tab[data-lens="attention"] .attention-badge');
    const label = badge?.getAttribute("aria-label") ?? "";
    expect(label).toMatch(/input/);
    expect(label).toContain("advisory");
  });

  it("renders a transient signed changed delta, never a new-events label", () => {
    store.commit({
      attention: attentionOf([item("a", "primary")]),
      attentionDelta: -2,
    });
    render.render();
    const delta = $(".attention-delta");
    expect(delta?.textContent).toBe("changed −2");
    expect(delta?.textContent).not.toContain("new");

    store.commit({ attentionDelta: 3 });
    render.render();
    expect($(".attention-delta")?.textContent).toBe("changed +3");

    store.commit({ attentionDelta: null });
    render.render();
    expect($(".attention-delta")).toBeNull();
  });
});

describe("renderSelected (delegates to detail)", () => {
  it("paints the event detail for a selected event", () => {
    store.commit({ selected: { kind: "event", id: OBS_EVENT }, open: true });
    render.render();
    const detail = $("#detail");
    expect(detail?.querySelector("dl.kv")).not.toBeNull();
    expect(detail?.textContent).toContain("the return value changed");
  });

  it("rests closed (single column) when nothing is selected", () => {
    store.commit({ selected: { kind: null, id: null } });
    render.render();
    expect($(".split")?.classList.contains("split-closed")).toBe(true);
  });

  it("collapses to a single column when the detail is closed", () => {
    store.commit({ selected: { kind: "event", id: OBS_EVENT }, open: false });
    render.render();
    expect($(".split")?.classList.contains("split-closed")).toBe(true);
    store.commit({ open: true });
    render.render();
    expect($(".split")?.classList.contains("split-closed")).toBe(false);
  });

  it("projects reading mode as a class on the split — only while open", () => {
    store.commit({
      selected: { kind: "event", id: OBS_EVENT },
      open: true,
      reading: true,
    });
    render.render();
    expect($(".split")?.classList.contains("reading")).toBe(true);
    store.commit({ open: false });
    render.render();
    expect($(".split")?.classList.contains("reading")).toBe(false);
  });

  it("the reading toggle enters reading mode and the rail leaves it", () => {
    store.commit({ selected: { kind: "event", id: OBS_EVENT }, open: true });
    render.render();
    ($("#detail-read") as HTMLElement).click();
    expect(store.getState().reading).toBe(true);
    ($("#master-rail") as HTMLElement).click();
    expect(store.getState().reading).toBe(false);
  });

  it("the reading toggle's glyph and label flip with the mode", () => {
    store.commit({ selected: { kind: "event", id: OBS_EVENT }, open: true });
    render.render();
    const btn = $("#detail-read") as HTMLElement;
    expect(btn.textContent).toBe("⤢");
    store.commit({ reading: true });
    render.render();
    expect(btn.textContent).toBe("⤡");
    expect(btn.getAttribute("aria-label")).toBe("Restore split");
  });

  it("the back affordance closes the detail keeping the cursor", () => {
    render.initControls();
    store.commit({ selected: { kind: "event", id: OBS_EVENT }, open: true });
    render.render();
    ($("#detail-back") as HTMLElement).click();
    expect(store.getState().open).toBe(false);
    expect(store.getState().selected.id).toBe(OBS_EVENT);
  });

  it("the close button closes the detail keeping the cursor", () => {
    store.commit({ selected: { kind: "event", id: OBS_EVENT }, open: true });
    render.render();
    ($("#detail-close") as HTMLElement).click();
    expect(store.getState().open).toBe(false);
    expect(store.getState().selected.id).toBe(OBS_EVENT);
  });

  it("a closed revision cursor does not fetch the composite", () => {
    const spy = vi.spyOn(globalThis, "fetch");
    store.commit({ selected: { kind: "revision", id: REV }, open: false });
    render.render();
    const urls = spy.mock.calls.map(([u]) => String(u));
    expect(urls.some((u) => u.includes("/api/revisions/"))).toBe(false);
    spy.mockRestore();
  });

  it("clicking a timeline row opens the detail", () => {
    render.render();
    const row = $("#master [data-event-id]") as HTMLElement;
    row.click();
    expect(store.getState().open).toBe(true);
    expect(store.getState().selected.kind).toBe("event");
  });
});

describe("the #master delegate (selection / open-diff / cue filter, ref-chip guard)", () => {
  it("selects an event on a timeline row click", () => {
    render.render();
    const row = $<HTMLElement>("#master #timeline .event[data-event-id]");
    expect(row).not.toBeNull();
    const id = row?.dataset.eventId;
    row?.dispatchEvent(new Event("click", { bubbles: true }));
    expect(store.getState().selected).toEqual({ kind: "event", id });
  });

  it("opens the snapshot diff on a list-card diff button click", () => {
    store.commit({ lens: "list" });
    render.render();
    const diffBtn = $<HTMLElement>("#master [data-open-diff]");
    expect(diffBtn?.dataset.openDiff).toBe(OBJ);
    diffBtn?.dispatchEvent(new Event("click", { bubbles: true }));
    expect(store.getState().diff).toBe(OBJ);
  });

  it("applies an attention-cue filter on click", () => {
    store.commit({ lens: "list" });
    render.render();
    const cue = $<HTMLElement>("#master [data-attention-query]");
    const query = cue?.dataset.attentionQuery;
    expect(query).toBeTruthy();
    cue?.dispatchEvent(new Event("click", { bubbles: true }));
    expect(store.getState().filterText).toBe(query);
  });

  it("lets ref chips fall through to the navigation delegate (no selection)", () => {
    render.render();
    const row = $<HTMLElement>("#master #timeline .event[data-event-id]");
    // A ref chip inside a selectable row must not trigger row selection — the
    // navigation delegate resolves data-ref-kind.
    const chip = document.createElement("span");
    chip.setAttribute("data-ref-kind", "rev");
    row?.appendChild(chip);
    chip.dispatchEvent(new Event("click", { bubbles: true }));
    expect(store.getState().selected.id).toBeNull();
  });
});

describe("scrollSelectionIntoView materializes an off-screen virtual row", () => {
  // Build enough entries that the timeline virtualizes; only a window is in the
  // DOM at a time, so selecting an off-screen entry must scroll its index into
  // the window before it can be revealed.
  function seedManyAndVirtualize(): HTMLElement {
    const entries = Array.from({ length: 500 }, (_, i) => ({
      eventId: `e${i}`,
      eventType: "review_observation_recorded",
      occurredAt: `unix-ms:${1782699185391 + i}`,
    }));
    store.commit({
      history: { entries, diagnostics: [] } as unknown as HistoryDoc,
      lens: "timeline",
    });
    render.render(); // creates #timeline and paints the top window
    const list = $<HTMLElement>("#timeline") as HTMLElement;
    Object.defineProperty(list, "clientHeight", {
      configurable: true,
      value: 500,
    });
    Object.defineProperty(list, "scrollTop", {
      configurable: true,
      writable: true,
      value: 0,
    });
    return list;
  }

  it("scrolls the selected off-screen event into the rendered window", () => {
    seedManyAndVirtualize();
    // The server-ordered page paints in array order, so the last row (e499) sits
    // far below the top window.
    const targetId = "e499";
    render.render();
    expect($(`#timeline li[data-event-id="${targetId}"]`)).toBeNull();

    store.commit({ selected: { kind: "event", id: targetId } });
    render.render();
    expect($(`#timeline li[data-event-id="${targetId}"]`)).not.toBeNull();
  });
});

describe("the diff page branch (no lens renders underneath)", () => {
  it("shows the page and hides the lens layout while diffPage is set", () => {
    render.render();
    expect($("#diff-page")?.classList.contains("hidden")).toBe(true);

    store.commit({ diffPage: true, diffRevision: REV });
    render.render();
    expect($("#diff-page")?.classList.contains("hidden")).toBe(false);
    expect($("#master")?.classList.contains("hidden")).toBe(true);
    expect($("#detail")?.classList.contains("hidden")).toBe(true);
    expect($("#toolbar")?.classList.contains("hidden")).toBe(true);
  });

  it("restores the lens layout when the page exits", () => {
    store.commit({ diffPage: true, diffRevision: REV });
    render.render();
    store.commit({ diffPage: false, diffRevision: null });
    render.render();
    expect($("#diff-page")?.classList.contains("hidden")).toBe(true);
    expect($("#master")?.classList.contains("hidden")).toBe(false);
    expect($("#detail")?.classList.contains("hidden")).toBe(false);
  });
});

describe("lens-aware search placeholder and query notices", () => {
  let router: typeof import("../src/router");

  beforeEach(async () => {
    router = await import("../src/router");
  });

  it("advertises event keys on the timeline and drops status:", () => {
    store.commit({ lens: "timeline" });
    render.render();
    const ph = $<HTMLInputElement>("#filter-text")?.placeholder ?? "";
    expect(ph).toContain("check:");
    expect(ph).not.toContain("status:");
  });

  it("advertises revision keys on the list lens", () => {
    store.commit({ lens: "list" });
    render.render();
    const ph = $<HTMLInputElement>("#filter-text")?.placeholder ?? "";
    expect(ph).toContain("attention:");
    // The revision index carries track/actor/is/tag slots, so their keys are
    // advertised; type:/check: stay event-only.
    expect(ph).toContain("track:");
    expect(ph).toContain("is:");
    expect(ph).not.toContain("type:");
    expect(ph).not.toContain("check:");
  });

  it("renders a client parse diagnostic in the route-diagnostic region", () => {
    store.commit({ lens: "timeline", filterText: "attention:x" });
    render.render();
    const diag = $("#route-diagnostic");
    expect(diag?.classList.contains("hidden")).toBe(false);
    expect(diag?.textContent ?? "").not.toBe("");
  });

  it("surfaces a server queryNotice from the history payload", () => {
    store.commit({
      lens: "timeline",
      filterText: "",
      history: {
        ...(historyJson as unknown as HistoryDoc),
        queryNotices: [
          {
            code: "deprecated-qualifier",
            key: "status",
            message: "use check:",
          },
        ],
      },
    });
    render.render();
    expect($("#route-diagnostic")?.textContent ?? "").toContain("use check:");
  });

  it("dedupes an equivalent client and server deprecation hint", () => {
    // The client parse of status:passed and a server queryNotice for the same clause
    // carry identical (code,key,message) — they must render once, not twice.
    const clientMsg = parseSearchQueryFor("status:passed", "event")
      .diagnostics[0].message;
    store.commit({
      lens: "timeline",
      filterText: "status:passed",
      history: {
        ...(historyJson as unknown as HistoryDoc),
        queryNotices: [
          { code: "deprecated-qualifier", key: "status", message: clientMsg },
        ],
      },
    });
    render.render();
    const hits = (
      $("#route-diagnostic")?.textContent?.match(/deprecated/g) ?? []
    ).length;
    expect(hits).toBe(1);
  });

  it("never clobbers a router-written diagnostic", () => {
    // A router message with no active query notice must survive the render.
    store.commit({ lens: "timeline", filterText: "" });
    router.showRouteDiagnostic("fell back to the timeline — unknown route /x");
    render.render();
    expect($("#route-diagnostic")?.textContent).toContain("unknown route");

    // A router message written AFTER a query notice is shown also survives.
    store.commit({ filterText: "attention:x" });
    render.render(); // shows the client parse notice
    router.showRouteDiagnostic("newer route diagnostic");
    render.render(); // must not overwrite the router's newer message
    expect($("#route-diagnostic")?.textContent).toContain(
      "newer route diagnostic",
    );
  });
});
