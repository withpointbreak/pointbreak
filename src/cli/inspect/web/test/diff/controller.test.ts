import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HistoryDoc, RevisionsDoc } from "../../src/store";
import historyJson from "../fixtures/history.json";
import revisionJson from "../fixtures/revision.json";
import revisionsJson from "../fixtures/revisions.json";
import { mountInspectorDom, resetDom } from "../support/dom";
import {
  installFetchMock,
  resetCompositeResponse,
  resetSnapshotResponse,
  setCompositeResponse,
  setSnapshotResponse,
  uninstallFetchMock,
} from "../support/fetch";

// `diff/controller.ts` owns the diff overlay lifecycle. It opens through the
// overlay teardown manager (registering its own `onClose`, importing no sibling
// overlay — the import-cycle cut), fetches the artifact through the `http` leaf,
// paints it via the pure `diff/render.renderDiff` (consuming the returned
// `{ html, ctx }`), and clears the route through `router.navigate` — never calling
// render (INV-6; the store subscriber repaints). The diff cursors / `diffCtx` /
// nav filter stay module-local. The store, the controller, and the overlay manager
// are module singletons, so reset the registry and re-import them before each test.
type Store = typeof import("../../src/store");
type Overlay = typeof import("../../src/overlay");
type Controller = typeof import("../../src/diff/controller");
let store: Store;
let overlay: Overlay;
let controller: Controller;

const REV = revisionsJson.entries[0].revisionId;
const OBJ = revisionsJson.entries[0].snapshotId;
const ARTIFACT = revisionsJson.entries[0].snapshotContentHash;
const COMPOSITE_FACT_IDS = [
  ...revisionJson.observations,
  ...revisionJson.inputRequests,
  ...revisionJson.assessments,
  ...revisionJson.validationChecks,
].map((fact) => fact.id);

function renderedFactIds(): string[] {
  return Array.from(
    document.querySelectorAll<HTMLElement>("#diff-page-body .anno[data-anno]"),
    (element) => element.dataset.anno ?? "",
  ).sort();
}

beforeEach(async () => {
  vi.resetModules();
  store = await import("../../src/store");
  overlay = await import("../../src/overlay");
  controller = await import("../../src/diff/controller");
  mountInspectorDom();
  installFetchMock();
  history.replaceState(null, "", "/");
  store.commit({
    history: historyJson as unknown as HistoryDoc,
    revisions: revisionsJson as unknown as RevisionsDoc,
  });
  controller.initControls();
});

afterEach(() => {
  uninstallFetchMock();
  resetCompositeResponse();
  resetSnapshotResponse();
  resetDom();
});

/** A synthetic artifact with `n` content-bearing files (more than the open budget). */
function syntheticArtifact(n: number): unknown {
  const files = [];
  for (let i = 0; i < n; i++) {
    files.push({
      status: "modified",
      old_path: `src/f${i}.rs`,
      new_path: `src/f${i}.rs`,
      metadata_rows: [],
      hunks: [
        {
          header: `@@ file ${i} @@`,
          rows: [
            { kind: "added", old_line: null, new_line: 1, text: `line ${i}` },
          ],
        },
      ],
    });
  }
  return { snapshot: { files } };
}

async function openCommitted(): Promise<void> {
  store.commit({ diffPage: true, diffRevision: REV, focus: null });
  await controller.renderDiffPage();
}

describe("openDiff / openRevisionDiff (page navigations — route-only, the paint is the reconciler's job)", () => {
  it("openDiff lands on the revision-primary page when the snapshot maps", () => {
    const push = vi.spyOn(history, "pushState");
    try {
      controller.openDiff(OBJ, null, ARTIFACT);
      expect(store.getState().diffPage).toBe(true);
      expect(store.getState().diffRevision).toBe(REV);
      expect(store.getState().diff).toBe(OBJ); // payload pointer retained
      expect(store.getState().diffHash).toBe(ARTIFACT);
      expect(push).toHaveBeenCalledTimes(1); // open = push
    } finally {
      push.mockRestore();
    }
  });

  it("openDiff lands on the snapshot-only page when nothing maps", () => {
    controller.openDiff("obj:sha256:unmapped", null, null);
    expect(store.getState().diffPage).toBe(true);
    expect(store.getState().diffRevision).toBeNull();
    expect(store.getState().diff).toBe("obj:sha256:unmapped");
  });

  it("openRevisionDiff opens the page on the revision's own identity", () => {
    controller.openRevisionDiff(REV, "obs:focus");
    expect(store.getState().diffPage).toBe(true);
    expect(store.getState().diffRevision).toBe(REV);
    expect(store.getState().focus).toBe("obs:focus");
  });

  it("openRevisionDiff needs no list entry (the composite resolves the snapshot)", () => {
    store.commit({ revisions: { entries: [] } as unknown as RevisionsDoc });
    controller.openRevisionDiff(REV);
    expect(store.getState().diffPage).toBe(true);
    expect(store.getState().diffRevision).toBe(REV);
  });
});

describe("closeDiff (a real push back to the record, never a replace)", () => {
  it("pushes the cleared route and never touches the parked cursor", async () => {
    store.commit({ selected: { kind: "revision", id: REV }, open: true });
    controller.openDiff(OBJ, "obs:focus", ARTIFACT);
    store.commit({ diffFile: "src/lib.rs", diffFileQuery: "has:facts" });

    const pushSpy = vi.spyOn(history, "pushState");
    const replaceSpy = vi.spyOn(history, "replaceState");
    try {
      controller.closeDiff();
      expect(store.getState().diffPage).toBe(false);
      expect(store.getState().diffRevision).toBeNull();
      expect(store.getState().diff).toBeNull();
      expect(store.getState().diffHash).toBeNull();
      expect(store.getState().focus).toBeNull();
      expect(store.getState().diffFile).toBeNull();
      expect(store.getState().diffFileQuery).toBe("");
      expect(pushSpy).toHaveBeenCalledTimes(1); // close = push
      expect(replaceSpy).not.toHaveBeenCalled(); // never {replace: true}
      // The cursor (and its open pane) survives the round trip untouched.
      expect(store.getState().selected).toEqual({ kind: "revision", id: REV });
      expect(store.getState().open).toBe(true);
    } finally {
      pushSpy.mockRestore();
      replaceSpy.mockRestore();
    }
  });

  it("closes through the wired page close control", async () => {
    controller.openRevisionDiff(REV);
    await controller.renderDiffPage();
    document
      .querySelector("#diff-page-close")
      ?.dispatchEvent(new Event("click", { bubbles: true }));
    expect(store.getState().diffPage).toBe(false);
    expect(store.getState().diffRevision).toBeNull();
  });

  it("is a no-op when no diff surface is addressed (no junk history entry)", () => {
    const pushSpy = vi.spyOn(history, "pushState");
    try {
      controller.closeDiff();
      expect(pushSpy).not.toHaveBeenCalled();
    } finally {
      pushSpy.mockRestore();
    }
  });
});

describe("lazy file bodies", () => {
  it("fills a collapsed file body on first expand and toggles its disclosure state", async () => {
    setSnapshotResponse(syntheticArtifact(12));
    await openCommitted();
    const collapsed = document.querySelector<HTMLElement>(
      '#diff-page-body .dfile[data-dfile="11"]',
    );
    expect(collapsed).not.toBeNull();
    const body = collapsed?.querySelector<HTMLElement>("[data-dfile-body]");
    expect(body?.dataset.rendered).toBeUndefined();
    expect(
      collapsed?.querySelector(".dfile-head")?.getAttribute("aria-expanded"),
    ).toBe("false");

    if (collapsed) controller.toggleDiffFile(collapsed);
    expect(
      collapsed?.querySelector(".dfile-head")?.getAttribute("aria-expanded"),
    ).toBe("true");
    expect(body?.dataset.rendered).toBe("1");
    expect(body?.innerHTML).toContain("dhunk");

    // Toggling again collapses without re-rendering the (already filled) body.
    if (collapsed) controller.toggleDiffFile(collapsed);
    expect(
      collapsed?.querySelector(".dfile-head")?.getAttribute("aria-expanded"),
    ).toBe("false");
    expect(body?.dataset.rendered).toBe("1");
  });
});

describe("the file/fact navigator (query-driven, no filter buttons)", () => {
  it("renders a summary, files, and the Decision context navigator section", async () => {
    await openCommitted();
    const nav = document.querySelector("#diff-page-nav-list");
    expect(nav?.querySelector("[data-diff-nav-filter]")).toBeNull(); // the button row is gone
    expect(nav?.querySelectorAll(".diff-nav-file").length).toBe(1);
    expect(renderedFactIds()).toEqual([...COMPOSITE_FACT_IDS].sort());
    // Revision-level facts and validation remain reachable in their named
    // context section, without consulting the loaded history window.
    expect(nav?.querySelector(".diff-decision-context-nav")).not.toBeNull();
    expect(
      nav?.querySelectorAll(".diff-decision-context-nav .diff-nav-fact"),
    ).toHaveLength(5);
    expect(nav?.querySelector(".diff-unanchored")).toBeNull();
  });

  it("typing has:facts narrows the file list the same way the old with-facts filter did", async () => {
    await openCommitted();
    store.commit({ diffFileQuery: "has:facts" });
    await controller.renderDiffPage();
    const nav = document.querySelector("#diff-page-nav-list");
    expect(nav?.querySelectorAll(".diff-nav-file").length).toBe(1);
  });

  it("Decision context stays visible when the file query narrows the file list to nothing", async () => {
    await openCommitted();
    store.commit({ diffFileQuery: "path:does-not-exist" });
    await controller.renderDiffPage();
    const nav = document.querySelector("#diff-page-nav-list");
    expect(nav?.querySelectorAll(".diff-nav-file").length).toBe(0);
    expect(nav?.querySelector(".diff-decision-context-nav")).not.toBeNull();
  });

  it("a Decision context navigator click follows the shared focus route", async () => {
    await openCommitted();
    const button = document.querySelector<HTMLElement>(
      ".diff-decision-context-nav .diff-nav-fact[data-anno]",
    );
    expect(button).not.toBeNull();
    button?.dispatchEvent(new Event("click", { bubbles: true }));
    expect(store.getState().focus).toBe(button?.dataset.anno);
  });

  it("keeps a missing-file target in Unanchored facts with its reason", async () => {
    const response = structuredClone(revisionJson);
    response.observations[0].target.filePath = "src/missing.rs";
    setCompositeResponse(response);
    await openCommitted();
    const nav = document.querySelector("#diff-page-nav-list .diff-unanchored");
    expect(nav?.textContent).toContain("file missing from snapshot");
    const button = nav?.querySelector<HTMLElement>(".diff-nav-fact[data-anno]");
    button?.dispatchEvent(new Event("click", { bubbles: true }));
    expect(store.getState().focus).toBe(response.observations[0].id);
    expect(
      document.querySelector("#diff-page-body .diff-unanchored-facts"),
    ).not.toBeNull();
  });

  it("nests responses under one request without adding response navigator facts", async () => {
    const response = structuredClone(revisionJson) as unknown as {
      inputRequests: Array<{
        id: string;
        status: string;
        responses: Array<{
          id: string;
          outcome: string;
          reason: string;
          createdAt: string;
          writer: { actorId: string };
        }>;
      }>;
    };
    const responseId = "input-request-response:sha256:nested";
    response.inputRequests[0].status = "responded";
    response.inputRequests[0].responses = [
      {
        id: responseId,
        outcome: "approved",
        reason: "ready to proceed",
        createdAt: "2026-07-17T08:00:00Z",
        writer: { actorId: "actor:agent:reviewer" },
      },
    ];
    setCompositeResponse(response);
    await openCommitted();
    const request = document.querySelector(
      `.diff-decision-context .anno[data-anno="${response.inputRequests[0].id}"]`,
    );
    expect(request?.querySelector(".fact-response")?.textContent).toContain(
      "ready to proceed",
    );
    expect(
      document.querySelector(`.diff-nav-fact[data-anno="${responseId}"]`),
    ).toBeNull();
  });
});

// The page owns its keyboard through the global layer's diff-page block (no
// overlay is involved — activeName() stays null): ]/[/n/p and Escape run;
// every lens, selection, paging, and lens-switch key is inert; with no page
// active the jump keys are dead.
describe("diff page keyboard + history", () => {
  const EVT_ON_REV = (
    historyJson as {
      entries: Array<{ eventId?: string; subject?: { revisionId?: string } }>;
    }
  ).entries.find((e) => e.subject?.revisionId === REV)?.eventId as string;

  let onKey: (ev: KeyboardEvent) => void;

  beforeEach(async () => {
    const keyboard = await import("../../src/keyboard");
    onKey = keyboard.onKey;
    document.addEventListener("keydown", onKey);
  });

  afterEach(() => {
    document.removeEventListener("keydown", onKey);
  });

  function press(key: string): KeyboardEvent {
    const ev = new KeyboardEvent("keydown", {
      key,
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(ev);
    return ev;
  }

  it("jumps to the next change on ']' while the page is active", async () => {
    await openCommitted();
    expect(overlay.activeName()).toBe(null); // a page, not an overlay
    const scrollSpy = vi
      .spyOn(Element.prototype, "scrollIntoView")
      .mockImplementation(() => {});
    try {
      const ev = press("]");
      expect(ev.defaultPrevented).toBe(true);
      const jumped = scrollSpy.mock.instances.at(-1);
      expect(
        jumped instanceof HTMLElement && jumped.classList.contains("dhunk"),
      ).toBe(true);
    } finally {
      scrollSpy.mockRestore();
    }
  });

  it("jumps to the next review fact on 'n', syncing the focus route", async () => {
    await openCommitted();
    press("n");
    const firstAnno = document.querySelector<HTMLElement>(
      "#diff-page-body .anno[data-anno]",
    );
    expect(firstAnno).not.toBeNull();
    expect(store.getState().focus).toBe(firstAnno?.dataset.anno);
  });

  it("keeps the whole lens key family inert on the page", async () => {
    await openCommitted();
    const before = structuredClone(store.getState());
    const pushSpy = vi.spyOn(history, "pushState");
    const replaceSpy = vi.spyOn(history, "replaceState");
    try {
      for (const k of [
        "j",
        "k",
        "1",
        "2",
        "3",
        "g",
        "G",
        "f",
        "b",
        "d",
        "u",
        "/",
        "Enter",
      ]) {
        press(k);
      }
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(store.getState()).toEqual(before); // no store commit
      expect(pushSpy).not.toHaveBeenCalled(); // no navigate...
      expect(replaceSpy).not.toHaveBeenCalled(); // ...not even a refinement
    } finally {
      pushSpy.mockRestore();
      replaceSpy.mockRestore();
    }
  });

  it("closes the page on Escape with a push", async () => {
    await openCommitted();
    const pushSpy = vi.spyOn(history, "pushState");
    const replaceSpy = vi.spyOn(history, "replaceState");
    try {
      press("Escape");
      expect(store.getState().diffPage).toBe(false);
      expect(store.getState().diffRevision).toBeNull();
      expect(pushSpy).toHaveBeenCalledTimes(1);
      expect(replaceSpy).not.toHaveBeenCalled();
    } finally {
      pushSpy.mockRestore();
      replaceSpy.mockRestore();
    }
  });

  it("keeps ']' inert when no page is active", async () => {
    const before = structuredClone(store.getState());
    const scrollSpy = vi
      .spyOn(Element.prototype, "scrollIntoView")
      .mockImplementation(() => {});
    try {
      press("]");
      expect(store.getState()).toEqual(before);
      expect(scrollSpy).not.toHaveBeenCalled();
    } finally {
      scrollSpy.mockRestore();
    }
  });

  it("Enter descends an open EVENT selection onto the page, cursor untouched", async () => {
    // The event case proves the page's identity lives in diffRevision, never
    // in the selection: the parked cursor survives open and close.
    store.commit({ selected: { kind: "event", id: EVT_ON_REV }, open: true });
    const pushSpy = vi.spyOn(history, "pushState");
    try {
      press("Enter");
      expect(pushSpy).toHaveBeenCalledTimes(1); // open = push
      expect(store.getState().diffPage).toBe(true);
      expect(store.getState().diffRevision).toBe(REV);
      expect(store.getState().selected).toEqual({
        kind: "event",
        id: EVT_ON_REV,
      });
      expect(store.getState().open).toBe(true);
      press("Escape");
      expect(store.getState().diffPage).toBe(false);
      expect(store.getState().selected).toEqual({
        kind: "event",
        id: EVT_ON_REV,
      }); // cursor intact
      expect(store.getState().open).toBe(true);
    } finally {
      pushSpy.mockRestore();
    }
  });

  it("returns to the record with the cursor intact on browser Back", async () => {
    const router = await import("../../src/router");
    router.navigate({
      selected: { kind: "event", id: EVT_ON_REV },
      open: false,
    });
    const recordHash = location.hash; // the parked-cursor record address
    controller.openRevisionDiff(REV);
    expect(location.hash).toContain("/diff");
    // Simulate the browser Back: the prior entry's hash is restored and the
    // popstate listener re-applies the route.
    history.replaceState(null, "", recordHash);
    router.applyHash();
    expect(store.getState().diffPage).toBe(false);
    expect(store.getState().selected).toEqual({
      kind: "event",
      id: EVT_ON_REV,
    });
    expect(store.getState().open).toBe(false);
  });
});

describe("fact / change jump keys", () => {
  it("applies a Decision context ?focus= deep link through the shared scroll path", async () => {
    const validationId = revisionJson.validationChecks[0].id;
    store.commit({
      diffPage: true,
      diffRevision: REV,
      focus: validationId,
    });
    await controller.renderDiffPage();
    const target = document.querySelector<HTMLElement>(
      `.diff-decision-context .anno[data-anno="${validationId}"]`,
    );
    expect(target).not.toBeNull();
    expect(target?.classList.contains("anno-flash")).toBe(true);
  });

  it("jumpFact advances to the next fact and replaces the route focus", async () => {
    await openCommitted();
    const replaceSpy = vi.spyOn(history, "replaceState");
    controller.jumpFact(1);
    const first = document.querySelector<HTMLElement>(
      "#diff-page-body .anno[data-anno]",
    );
    expect(store.getState().focus).toBe(first?.dataset.anno);
    expect(replaceSpy).toHaveBeenCalled();
    replaceSpy.mockRestore();
  });

  it("jumpChange cycles change anchors without touching the focus route", async () => {
    await openCommitted();
    expect(
      document.querySelectorAll("#diff-page-body .dhunk").length,
    ).toBeGreaterThan(0);
    const focusBefore = store.getState().focus;
    controller.jumpChange(1);
    expect(store.getState().focus).toBe(focusBefore);
  });

  it("a noted gutter click scrolls to the annotation and syncs the focus route", async () => {
    await openCommitted();
    const noted = document.querySelector<HTMLElement>(
      "#diff-page-body .drow-noted[data-anno]",
    );
    expect(noted).not.toBeNull();
    noted?.dispatchEvent(new Event("click", { bubbles: true }));
    expect(store.getState().focus).toBe(noted?.dataset.anno);
  });
});

describe("the file-search query (?fq=)", () => {
  it("filters the nav file list through matchDiffFiles and clears on an empty query", async () => {
    setSnapshotResponse(syntheticArtifact(12));
    await openCommitted();
    const input = document.querySelector<HTMLInputElement>("#diff-file-query");
    expect(input).not.toBeNull();
    if (input) {
      input.value = "f11";
      input.dispatchEvent(new Event("input", { bubbles: true }));
    }
    expect(store.getState().diffFileQuery).toBe("f11");
    const nav = document.querySelector("#diff-page-nav-list");
    expect(nav?.querySelectorAll(".diff-nav-file").length).toBe(1);
  });

  it("syncs the input's value from route state on a cold diff-page load (?fq= deep link)", async () => {
    store.commit({
      diffPage: true,
      diffRevision: REV,
      diffFileQuery: "path:api",
    });
    await controller.renderDiffPage();
    const input = document.querySelector<HTMLInputElement>("#diff-file-query");
    expect(input?.value).toBe("path:api");
  });

  it("renders an inline diagnostic for an unsupported qualifier without emptying the list", async () => {
    await openCommitted();
    store.commit({ diffFileQuery: "status:modified" });
    await controller.renderDiffPage();
    const nav = document.querySelector("#diff-page-nav-list");
    expect(nav?.textContent).toContain("status:");
    expect(nav?.querySelectorAll(".diff-nav-file").length).toBeGreaterThan(0);
  });

  it("closing the page clears diffFileQuery (DIFF_ROUTE_CLEARED)", async () => {
    await openCommitted();
    store.commit({ diffFileQuery: "path:api" });
    controller.closeDiff();
    expect(store.getState().diffFileQuery).toBe("");
  });

  it("typing in the file-search input does not trigger n/p/]/[ file navigation", async () => {
    const keyboard = await import("../../src/keyboard");
    document.addEventListener("keydown", keyboard.onKey);
    try {
      await openCommitted();
      const input =
        document.querySelector<HTMLInputElement>("#diff-file-query");
      input?.focus();
      const scrollSpy = vi
        .spyOn(Element.prototype, "scrollIntoView")
        .mockImplementation(() => {});
      try {
        for (const key of ["n", "p", "]", "["]) {
          const ev = new KeyboardEvent("keydown", {
            key,
            bubbles: true,
            cancelable: true,
          });
          input?.dispatchEvent(ev);
          expect(ev.defaultPrevented).toBe(false);
        }
        expect(scrollSpy).not.toHaveBeenCalled();
      } finally {
        scrollSpy.mockRestore();
      }
    } finally {
      document.removeEventListener("keydown", keyboard.onKey);
    }
  });
});

// The routed diff page: a route surface (never an overlay — activeName() stays
// null) painted from `state.diffPage`/`diffRevision`. Facts AND snapshot identity
// come from the composite revision document, so cold and grouped-away deep links
// paint annotated; an unmappable snapshot-only link paints bytes with blank facts.
describe("renderDiffPage (the routed page surface)", () => {
  function pageBody(): HTMLElement | null {
    return document.querySelector<HTMLElement>("#diff-page-body");
  }

  it("paints facts on a cold revision-primary deep link (no history, no list)", async () => {
    // Cold: nothing loaded — only the composite + snapshot endpoints answer.
    store.commit({
      history: { entries: [], diagnostics: [] } as unknown as HistoryDoc,
      revisions: { entries: [] } as unknown as RevisionsDoc,
      diffPage: true,
      diffRevision: REV,
    });
    await controller.renderDiffPage();
    expect(pageBody()?.innerHTML).toContain("dfile");
    // The annotated half of "annotated diff": fact markers render from the
    // composite document, never the paged history or the list document.
    expect(pageBody()?.querySelectorAll(".anno[data-anno]").length).toBe(
      COMPOSITE_FACT_IDS.length,
    );
    expect(renderedFactIds()).toEqual([...COMPOSITE_FACT_IDS].sort());
    expect(document.querySelector("#diff-page-title")?.textContent).toContain(
      "snapshot",
    );
    expect(document.querySelector("#diff-page-title")?.textContent).toContain(
      revisionJson.revision.targetDisplay.workLabel.text,
    );
    // A route surface, not an overlay: the manager never owns the page.
    expect(overlay.activeName()).toBe(null);
  });

  it("keeps capture summary primary and derived work provenance visible", async () => {
    const response = structuredClone(revisionJson) as unknown as {
      revision: {
        summary?: string;
        targetDisplay: { workLabel: { text: string } };
      };
    };
    response.revision.summary = "Preserve the review decision";
    setCompositeResponse(response);
    store.commit({ diffPage: true, diffRevision: REV });
    await controller.renderDiffPage();
    const title = document.querySelector("#diff-page-title")?.textContent ?? "";
    expect(title.startsWith("Preserve the review decision")).toBe(true);
    expect(title).toContain(response.revision.targetDisplay.workLabel.text);
    expect(title).toContain("snapshot");
  });

  it("paints facts for a grouped-away revision absent from the loaded list", async () => {
    store.commit({
      revisions: { entries: [] } as unknown as RevisionsDoc, // grouped away
      diffPage: true,
      diffRevision: REV,
    });
    await controller.renderDiffPage();
    expect(pageBody()?.innerHTML).toContain("dfile");
    expect(pageBody()?.querySelectorAll(".anno[data-anno]").length).toBe(
      COMPOSITE_FACT_IDS.length,
    );
    expect(renderedFactIds()).toEqual([...COMPOSITE_FACT_IDS].sort());
  });

  it("paints best-effort blank for an unmappable snapshot-only link", async () => {
    store.commit({
      diffPage: true,
      diffRevision: null,
      diff: OBJ,
      diffHash: ARTIFACT,
    });
    await controller.renderDiffPage();
    expect(pageBody()?.innerHTML).toContain("dfile"); // the bytes render
    expect(pageBody()?.querySelectorAll(".anno[data-anno]").length).toBe(0);
    expect(pageBody()?.textContent).toContain("no review facts");
  });

  it("applies ?fq= from route state and expands the ?file= target", async () => {
    setSnapshotResponse(syntheticArtifact(12));
    store.commit({
      diffPage: true,
      diffRevision: REV,
      diffFileQuery: "has:facts",
      diffFile: "src/f11.rs",
    });
    await controller.renderDiffPage();
    const input = document.querySelector<HTMLInputElement>("#diff-file-query");
    expect(input?.value).toBe("has:facts");
    // The ?file= target's body is ensured and its section expanded.
    const section = pageBody()?.querySelector('.dfile[data-dfile="11"]');
    expect(
      section?.querySelector(".dfile-head")?.getAttribute("aria-expanded"),
    ).toBe("true");
    expect(
      section?.querySelector<HTMLElement>("[data-dfile-body]")?.innerHTML,
    ).toContain("dhunk");
  });

  it("re-paints idempotently on a freshness-poll repaint", async () => {
    store.commit({ diffPage: true, diffRevision: REV });
    await controller.renderDiffPage();
    expect(pageBody()?.innerHTML).toContain("dfile");
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const scrollSpy = vi
      .spyOn(Element.prototype, "scrollIntoView")
      .mockImplementation(() => {});
    try {
      await controller.renderDiffPage(); // unchanged state — the poll repaint
      expect(fetchSpy).not.toHaveBeenCalled(); // no duplicate fetch
      expect(scrollSpy).not.toHaveBeenCalled(); // no scroll reset
      expect(pageBody()?.innerHTML).toContain("dfile"); // still painted
    } finally {
      fetchSpy.mockRestore();
      scrollSpy.mockRestore();
    }
  });
});
