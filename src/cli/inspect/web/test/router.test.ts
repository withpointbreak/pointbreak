import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RoutePatch, SerializeSnapshot } from "../src/router";
import type { HistoryDoc, RevisionsDoc, ThreadsDoc } from "../src/store";
import { mountInspectorDom, resetDom } from "./support/dom";
import {
  installFetchMock,
  resetHistoryResponse,
  setHistoryResponse,
  uninstallFetchMock,
} from "./support/fetch";

// The router is the hash grammar plus the navigate/apply choke point. `parseHash`
// and `serializeState` are pure inverses (no global reads — `parseHash` takes the
// present types, `serializeState` takes a state snapshot), so the grammar is tested
// by round-trip without a store. `navigate`/`applyHash` mutate through `store.commit`
// and `history` and never call render — the store subscriber is the only repaint
// path (wired elsewhere). The store and the router are module singletons sharing one
// `state`, so reset the registry and re-import both before each test.
type Store = typeof import("../src/store");
type Router = typeof import("../src/router");
let store: Store;
let router: Router;

// A stable present-types view passed to both pure functions so the round-trip is
// deterministic (the default `enabledTypes` and the `types=` param both derive from
// it).
const PT = [
  "work_object_proposed",
  "review_observation_recorded",
  "review_assessment_recorded",
];

const REV =
  "rev:sha256:9a7626ca7cb2801721ed992402184460210477aadfd4f7228628b65ff11a6efd";
const EVT =
  "evt:sha256:1111111111111111111111111111111111111111111111111111111111111111";

beforeEach(async () => {
  vi.resetModules();
  store = await import("../src/store");
  router = await import("../src/router");
  // A deep link to an off-page event fetches its page to reveal it; the fixture
  // mock absorbs those reveal fetches.
  installFetchMock();
  // Each test starts from a clean fragment so `applyHash` reads what it sets.
  history.replaceState(null, "", "/");
});

afterEach(() => {
  resetHistoryResponse();
  uninstallFetchMock();
  resetDom();
});

/** Let the async fetch-to-reveal step `applyHash` kicks settle. */
function flush(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

/** The State-bound route fields a parsed patch carries (excludes the transient seam). */
function routeFields(p: RoutePatch) {
  return {
    lens: p.lens,
    selected: p.selected,
    open: p.open,
    filterTrack: p.filterTrack,
    filterSnapshot: p.filterSnapshot,
    order: p.order,
    sortKey: p.sortKey,
    enabledTypes: p.enabledTypes,
    filterText: p.filterText,
    diff: p.diff,
    diffHash: p.diffHash,
    focus: p.focus,
    diffPage: p.diffPage,
    diffRevision: p.diffRevision,
    diffFile: p.diffFile,
    diffNav: p.diffNav,
  };
}

/** A serialize snapshot built from a parsed patch (the inverse direction's input). */
function snapshotFrom(p: RoutePatch): SerializeSnapshot {
  return {
    lens: p.lens,
    // The diff-page path omits the cursor fields (it never touches the parked
    // cursor); serialization then reads the untouched defaults.
    selected: p.selected ?? { kind: null, id: null },
    open: p.open ?? false,
    filterTrack: p.filterTrack,
    filterSnapshot: p.filterSnapshot,
    order: p.order,
    sortKey: p.sortKey,
    enabledTypes: p.enabledTypes,
    filterText: p.filterText,
    diff: p.diff,
    diffHash: p.diffHash,
    focus: p.focus,
    diffPage: p.diffPage,
    diffRevision: p.diffRevision,
    diffFile: p.diffFile,
    diffNav: p.diffNav,
  };
}

describe("parseHash", () => {
  it("defaults an empty fragment to the timeline lens with no selection", () => {
    const p = router.parseHash("", PT);
    expect(p.lens).toBe("timeline");
    expect(p.selected).toEqual({ kind: null, id: null });
    expect(p.order).toBe("desc");
    expect(p.filterText).toBe("");
    expect(p.filterTrack).toBe("");
    expect(p.filterSnapshot).toBe("");
    expect(p.diff).toBeNull();
    expect(p.diffHash).toBeNull();
    expect(p.focus).toBeNull();
  });

  it("reads a bare lens path", () => {
    expect(router.parseHash("#/list", PT).lens).toBe("list");
    expect(router.parseHash("#/attention", PT).lens).toBe("attention");
  });

  it("reads `?sort=` on the list lens, defaulting to captured", () => {
    expect(router.parseHash("#/list", PT).sortKey).toBe("captured");
    expect(router.parseHash("#/list?sort=activity", PT).sortKey).toBe(
      "activity",
    );
  });

  it("ignores an invalid sort value, falling back to captured", () => {
    expect(router.parseHash("#/list?sort=sideways", PT).sortKey).toBe(
      "captured",
    );
  });

  it("aliases #/threads to the list lens preserving query params", () => {
    const patch = router.parseHash(
      `#/threads?track=trk&sel=${encodeURIComponent(REV)}&snapshot=obj:sha256:abc`,
      PT,
    );
    expect(patch.lens).toBe("list");
    expect(patch.unknownPath).toBeNull(); // never the unknown-route fallback
    expect(patch.migrated).toBe("threads-alias"); // the parse-seam flag
    expect(patch.filterTrack).toBe("trk");
    expect(patch.selected).toEqual({ kind: "revision", id: REV });
    expect(patch.filterSnapshot).toBe("obj:sha256:abc");
  });

  it("defaults the enabled types to the present types when `types=` is absent", () => {
    expect(router.parseHash("#/timeline", PT).enabledTypes).toEqual(
      new Set(PT),
    );
  });

  it("reads an explicit `types=` list", () => {
    expect(router.parseHash("#/timeline?types=a,b", PT).enabledTypes).toEqual(
      new Set(["a", "b"]),
    );
  });

  it("selects an entity-primary revision path", () => {
    const p = router.parseHash(`#/revision/${encodeURIComponent(REV)}`, PT);
    expect(p.selected).toEqual({ kind: "revision", id: REV });
    expect(p.lens).toBe("timeline");
  });

  it("selects an entity-primary event path", () => {
    const p = router.parseHash(`#/event/${encodeURIComponent(EVT)}`, PT);
    expect(p.selected).toEqual({ kind: "event", id: EVT });
  });

  it("keeps the master lens behind an entity-primary path via `?lens=`", () => {
    const p = router.parseHash(
      `#/revision/${encodeURIComponent(REV)}?lens=list`,
      PT,
    );
    expect(p.selected).toEqual({ kind: "revision", id: REV });
    expect(p.lens).toBe("list");
  });

  it("reads a lens-primary selection via `?sel=` and classifies its kind", () => {
    const p = router.parseHash(`#/list?sel=${encodeURIComponent(REV)}`, PT);
    expect(p.lens).toBe("list");
    expect(p.selected).toEqual({ kind: "revision", id: REV });
    // A non-revision id classifies as an event selection.
    const q = router.parseHash(`#/list?sel=${encodeURIComponent(EVT)}`, PT);
    expect(q.selected).toEqual({ kind: "event", id: EVT });
  });

  it("reads the cross-lens scope, order, and query params", () => {
    const p = router.parseHash(
      "#/timeline?track=human:kevin&snapshot=obj:1&order=asc&q=hello%20world",
      PT,
    );
    expect(p.filterTrack).toBe("human:kevin");
    expect(p.filterSnapshot).toBe("obj:1");
    expect(p.order).toBe("asc");
    expect(p.filterText).toBe("hello world");
  });

  it("still parses the legacy object= param into the snapshot filter (#334 grace)", () => {
    expect(router.parseHash("#/timeline?object=obj:1", PT).filterSnapshot).toBe(
      "obj:1",
    );
  });

  it("ignores an invalid order, falling back to desc", () => {
    expect(router.parseHash("#/timeline?order=sideways", PT).order).toBe(
      "desc",
    );
  });

  it("reads the route-preserving diff overlay params", () => {
    const p = router.parseHash(
      "#/timeline?diff=obj:1&diffHash=sha256:abc&focus=evt:9",
      PT,
    );
    expect(p.diff).toBe("obj:1");
    expect(p.diffHash).toBe("sha256:abc");
    expect(p.focus).toBe("evt:9");
  });

  it("flags an unknown path for the resolve fallback", () => {
    const p = router.parseHash("#/bogus", PT);
    expect(p.lens).toBe("timeline");
    expect(p.unknownPath).toBe("/bogus");
  });

  it("treats a diff path as unknown — diff stays an overlay, never a master lens", () => {
    const p = router.parseHash("#/diff/obj:1", PT);
    expect(p.unknownPath).toBe("/diff/obj:1");
  });

  it("rejects an unrecognized lens behind an entity path, keeping the default lens", () => {
    const p = router.parseHash(
      `#/revision/${encodeURIComponent(REV)}?lens=diff`,
      PT,
    );
    expect(p.lens).toBe("timeline");
  });

  it("ignores the reserved `v=` grammar version param", () => {
    // `v=1` is a documented forward-compat marker the parser deliberately ignores.
    expect(routeFields(router.parseHash("#/list?v=1", PT))).toEqual(
      routeFields(router.parseHash("#/list", PT)),
    );
  });

  it("records reserved journal/asof links as unsupported live-state input", () => {
    const p = router.parseHash("#/timeline?journal=main&asof=2026", PT);
    expect(p.unsupportedJournal).not.toBeNull();
    expect(p.unsupportedAsOf).not.toBeNull();
  });

  it("parses entity-primary as open and lens-primary ?sel= as a parked cursor", () => {
    expect(
      router.parseHash(`#/event/${encodeURIComponent(EVT)}`, PT).open,
    ).toBe(true);
    expect(
      router.parseHash(`#/revision/${encodeURIComponent(REV)}`, PT).open,
    ).toBe(true);
    expect(
      router.parseHash(`#/timeline?sel=${encodeURIComponent(EVT)}`, PT).open,
    ).toBe(false);
    expect(router.parseHash("#/timeline", PT).open).toBe(false);
  });
});

describe("serializeState", () => {
  function snap(over: Partial<SerializeSnapshot> = {}): SerializeSnapshot {
    return {
      lens: "timeline",
      selected: { kind: null, id: null },
      open: false,
      filterTrack: "",
      filterSnapshot: "",
      order: "desc",
      sortKey: "captured",
      enabledTypes: new Set(PT),
      filterText: "",
      diff: null,
      diffHash: null,
      focus: null,
      diffPage: false,
      diffRevision: null,
      diffFile: null,
      diffNav: "all",
      ...over,
    };
  }

  it("serializes the default lens without params", () => {
    expect(router.serializeState(snap(), PT)).toBe("#/timeline");
  });

  it("emits sort= only on the list lens and only when non-default", () => {
    expect(
      router.serializeState(snap({ lens: "list", sortKey: "activity" }), PT),
    ).toContain("sort=activity");
    expect(
      router.serializeState(snap({ lens: "list", sortKey: "captured" }), PT),
    ).not.toContain("sort=");
    // Off-lens elision: a non-default sortKey stays in memory, but the timeline
    // never round-trips it — the same deliberate elision `order` gets.
    expect(
      router.serializeState(
        snap({ lens: "timeline", sortKey: "activity" }),
        PT,
      ),
    ).not.toContain("sort=");
  });

  it("serializes a non-default lens", () => {
    expect(router.serializeState(snap({ lens: "attention" }), PT)).toBe(
      "#/attention",
    );
  });

  it("serializes an entity-primary revision selection, carrying a non-default lens", () => {
    expect(
      router.serializeState(
        snap({
          lens: "list",
          selected: { kind: "revision", id: REV },
          open: true,
        }),
        PT,
      ),
    ).toBe(`#/revision/${encodeURIComponent(REV)}?lens=list`);
  });

  it("omits the lens param when the selection sits on the default lens", () => {
    expect(
      router.serializeState(
        snap({ selected: { kind: "event", id: EVT }, open: true }),
        PT,
      ),
    ).toBe(`#/event/${encodeURIComponent(EVT)}`);
  });

  it("serializes the filters, order, query, and diff overlay", () => {
    expect(
      router.serializeState(
        snap({
          filterTrack: "human:kevin",
          filterSnapshot: "obj:1",
          order: "asc",
          filterText: "hello world",
          diff: "obj:1",
          diffHash: "sha256:abc",
          focus: "evt:9",
        }),
        PT,
      ),
    ).toBe(
      "#/timeline?track=human%3Akevin&snapshot=obj%3A1&order=asc&q=hello%20world&diff=obj%3A1&diffHash=sha256%3Aabc&focus=evt%3A9",
    );
  });

  it("emits a `types=` param only when some present type is disabled", () => {
    // All present types enabled -> omit.
    expect(router.serializeState(snap(), PT)).not.toContain("types=");
    // One disabled -> emit only the enabled ids.
    expect(
      router.serializeState(snap({ enabledTypes: new Set([PT[0]]) }), PT),
    ).toContain(`types=${PT[0]}`);
  });

  it("drops a diffHash when no diff is open", () => {
    expect(
      router.serializeState(snap({ diff: null, diffHash: "sha256:abc" }), PT),
    ).toBe("#/timeline");
  });

  it("serializes a parked cursor as lens-primary ?sel=", () => {
    expect(
      router.serializeState(
        snap({ selected: { kind: "event", id: EVT }, open: false }),
        PT,
      ),
    ).toBe(`#/timeline?sel=${encodeURIComponent(EVT)}`);
    // A parked revision cursor on a non-default lens keeps the lens path.
    expect(
      router.serializeState(
        snap({
          lens: "list",
          selected: { kind: "revision", id: REV },
          open: false,
        }),
        PT,
      ),
    ).toBe(`#/list?sel=${encodeURIComponent(REV)}`);
  });

  it("never serializes the reading flag — session-only, byte-identical URLs", () => {
    const base = snap({ selected: { kind: "event", id: EVT }, open: true });
    const withReading = {
      ...base,
      reading: true,
    } as SerializeSnapshot & { reading: boolean };
    expect(router.serializeState(withReading, PT)).toBe(
      router.serializeState(base, PT),
    );
    expect("reading" in router.parseHash(`#/event/${EVT}`, PT)).toBe(false);
  });

  it("serializes an open selection entity-primary", () => {
    expect(
      router.serializeState(
        snap({ selected: { kind: "event", id: EVT }, open: true }),
        PT,
      ),
    ).toBe(`#/event/${encodeURIComponent(EVT)}`);
  });
});

describe("grammar round-trip (parseHash and serializeState are inverses)", () => {
  const hashes = [
    "#/timeline",
    "#/list",
    "#/attention",
    `#/revision/${encodeURIComponent(REV)}`,
    `#/revision/${encodeURIComponent(REV)}?lens=list`,
    `#/event/${encodeURIComponent(EVT)}`,
    "#/timeline?track=human:kevin&snapshot=obj:1&order=asc&q=needle",
    "#/timeline?diff=obj:1&diffHash=sha256:abc&focus=evt:9",
    // A subset of the present types — serializeState only re-emits present ids.
    `#/timeline?types=${PT[0]},${PT[1]}`,
    // A parked cursor (lens-primary selection, pane closed).
    `#/timeline?sel=${encodeURIComponent(EVT)}`,
    `#/list?sel=${encodeURIComponent(REV)}`,
    // The routed diff page (bare, and with its full param set).
    `#/revision/${encodeURIComponent(REV)}/diff`,
    `#/revision/${encodeURIComponent(REV)}/diff?focus=evt:9&file=src%2Fmain.rs&nav=unanchored`,
  ];

  for (const hash of hashes) {
    it(`round-trips ${hash}`, () => {
      const first = router.parseHash(hash, PT);
      const reserialized = router.serializeState(snapshotFrom(first), PT);
      const second = router.parseHash(reserialized, PT);
      expect(routeFields(second)).toEqual(routeFields(first));
    });
  }
});

describe("diff page route", () => {
  const OBJ = "obj:1";
  const FACT =
    "obs:sha256:2222222222222222222222222222222222222222222222222222222222222222";

  function seedDiff(): void {
    store.commit({
      history: {
        entries: [{ eventId: EVT, eventType: "work_object_proposed" }],
        diagnostics: [],
      } as unknown as HistoryDoc,
      revisions: {
        entries: [{ revisionId: REV, snapshotId: OBJ }],
      } as unknown as RevisionsDoc,
      threads: {
        threads: [{ revisions: [REV] }],
        revisionClassification: {},
      } as unknown as ThreadsDoc,
    });
  }

  it("round-trips the canonical diff-page path", () => {
    const hash = `#/revision/${encodeURIComponent(REV)}/diff?focus=${encodeURIComponent(FACT)}&file=${encodeURIComponent("src/cli/inspect/api.rs")}&nav=with-facts`;
    const patch = router.parseHash(hash, PT);
    expect(patch.diffPage).toBe(true);
    expect(patch.diffRevision).toBe(REV); // the page's own identity
    expect(patch.selected).toBeUndefined(); // never touches the parked cursor
    expect(patch.open).toBeUndefined();
    expect(patch.focus).toBe(FACT);
    expect(patch.diffFile).toBe("src/cli/inspect/api.rs");
    expect(patch.diffNav).toBe("with-facts");
    expect(router.serializeState(snapshotFrom(patch), PT)).toBe(hash);
  });

  it("copies sortKey unconditionally, even on the diff-page patch that omits selected/open", () => {
    const patch = router.parseHash(
      `#/revision/${encodeURIComponent(REV)}/diff`,
      PT,
    );
    expect(patch.selected).toBeUndefined(); // the omission this guards against
    expect(patch.sortKey).toBe("captured"); // present regardless of the path arm
    const next = router.resolve(patch);
    expect(next.sortKey).toBe("captured");
  });

  it("omits nav at its default and rejects a garbage nav value", () => {
    const bare = router.parseHash(
      `#/revision/${encodeURIComponent(REV)}/diff`,
      PT,
    );
    expect(bare.diffNav).toBe("all");
    expect(router.serializeState(snapshotFrom(bare), PT)).toBe(
      `#/revision/${encodeURIComponent(REV)}/diff`,
    );
    const garbage = router.parseHash(
      `#/revision/${encodeURIComponent(REV)}/diff?nav=bogus`,
      PT,
    );
    expect(garbage.diffNav).toBe("all");
    expect(router.serializeState(snapshotFrom(garbage), PT)).not.toContain(
      "nav=",
    );
  });

  it("never emits the legacy diff params alongside the canonical path", () => {
    const s = snapshotFrom(
      router.parseHash(`#/revision/${encodeURIComponent(REV)}/diff`, PT),
    );
    s.diff = OBJ;
    s.diffHash = "sha256:abc";
    expect(router.serializeState(s, PT)).toBe(
      `#/revision/${encodeURIComponent(REV)}/diff`,
    );
  });

  it("serializes a snapshot-only page in the legacy query form", () => {
    const s = snapshotFrom(router.parseHash("#/timeline", PT));
    s.diffPage = true;
    s.diff = OBJ;
    s.diffHash = "sha256:abc";
    expect(router.serializeState(s, PT)).toBe(
      `#/timeline?diff=${encodeURIComponent(OBJ)}&diffHash=${encodeURIComponent("sha256:abc")}`,
    );
  });

  it("forwards a legacy ?diff= link to the canonical path when the snapshot maps", async () => {
    seedDiff();
    mountInspectorDom();
    const push = vi.spyOn(history, "pushState");
    try {
      history.replaceState(
        null,
        "",
        `#/timeline?diff=${encodeURIComponent(OBJ)}`,
      );
      router.applyHash();
      await flush();
      expect(store.getState().diffPage).toBe(true);
      expect(store.getState().diffRevision).toBe(REV);
      expect(store.getState().diff).toBe(OBJ); // payload pointer retained
      expect(store.getState().selected).toEqual({ kind: null, id: null });
      // The address bar is replace-rewritten to the canonical page path;
      // replace, never push, so Back does not bounce through the stale form.
      expect(
        location.hash.startsWith(`#/revision/${encodeURIComponent(REV)}/diff`),
      ).toBe(true);
      expect(push).not.toHaveBeenCalled();
    } finally {
      push.mockRestore();
    }
  });

  it("keeps an unmappable snapshot-only ?diff= snapshot-addressed with no rewrite", async () => {
    seedDiff();
    mountInspectorDom();
    history.replaceState(null, "", "#/timeline?diff=obj:unknown");
    router.applyHash();
    await flush();
    // Every legacy diff intent renders as the page, but the route must not
    // invent a revision — and an unmapped link keeps its original address.
    expect(store.getState().diffPage).toBe(true);
    expect(store.getState().diff).toBe("obj:unknown");
    expect(store.getState().diffRevision).toBeNull();
    expect(location.hash).toBe("#/timeline?diff=obj:unknown");
  });

  it("applies a canonical diff hash over a parked cursor without touching it", async () => {
    seedDiff();
    mountInspectorDom();
    store.commit({ selected: { kind: "event", id: EVT }, open: true });
    history.replaceState(
      null,
      "",
      `#/revision/${encodeURIComponent(REV)}/diff`,
    );
    router.applyHash();
    await flush();
    expect(store.getState().diffPage).toBe(true);
    expect(store.getState().diffRevision).toBe(REV);
    expect(store.getState().selected).toEqual({ kind: "event", id: EVT });
    expect(store.getState().open).toBe(true);
  });

  it("keeps a diffRevision that is absent from the loaded list", () => {
    // Grouped-away ids resolve through the entity-primary composite fetch; the
    // route must not clear them or fall back with a diagnostic.
    store.commit({
      history: { entries: [], diagnostics: [] } as unknown as HistoryDoc,
      revisions: { entries: [] } as unknown as RevisionsDoc,
      threads: {
        threads: [],
        revisionClassification: {},
      } as unknown as ThreadsDoc,
    });
    mountInspectorDom();
    history.replaceState(
      null,
      "",
      `#/revision/${encodeURIComponent(REV)}/diff`,
    );
    router.applyHash();
    expect(store.getState().diffPage).toBe(true);
    expect(store.getState().diffRevision).toBe(REV);
    expect(
      document.querySelector("#route-diagnostic")?.classList.contains("hidden"),
    ).toBe(true);
  });
});

describe("selectionKind", () => {
  it("classifies a rev: id as a revision selection", () => {
    expect(router.selectionKind(REV)).toBe("revision");
  });

  it("preserves the legacy review-unit: id as a revision selection", () => {
    expect(
      router.selectionKind(
        "review-unit:sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
      ),
    ).toBe("revision");
  });

  it("classifies anything else as an event selection", () => {
    expect(router.selectionKind(EVT)).toBe("event");
    expect(router.selectionKind("obj:sha256:abc")).toBe("event");
  });
});

describe("parseQuery", () => {
  it("decodes an &-separated key/value list", () => {
    expect(router.parseQuery("a=1&b=two%20words")).toEqual({
      a: "1",
      b: "two words",
    });
  });

  it("treats a bare key as an empty value and skips empty pairs", () => {
    expect(router.parseQuery("flag&&x=1")).toEqual({ flag: "", x: "1" });
  });
});

describe("routeDiagnostic", () => {
  it("joins a primary and secondary message", () => {
    expect(router.routeDiagnostic("primary", "secondary")).toBe(
      "primary — secondary",
    );
  });

  it("returns the primary alone when there is no secondary", () => {
    expect(router.routeDiagnostic("primary", "")).toBe("primary");
  });
});

describe("liveStateDiagnostic", () => {
  it("reports unsupported as-of and journal links without mutating the patch", () => {
    const p = router.parseHash("#/timeline?journal=main&asof=2026", PT);
    const msg = router.liveStateDiagnostic(p);
    expect(msg).toContain("as-of links are not supported");
    expect(msg).toContain("journal links are not supported");
    // The seam fields stay on the patch — they are dropped by resolve building a
    // clean state patch, not by deleting them here.
    expect(p.unsupportedJournal).not.toBeNull();
    expect(p.unsupportedAsOf).not.toBeNull();
  });

  it("returns an empty string when nothing reserved is present", () => {
    expect(router.liveStateDiagnostic(router.parseHash("#/timeline", PT))).toBe(
      "",
    );
  });
});

describe("route diagnostics DOM", () => {
  beforeEach(() => {
    mountInspectorDom();
  });

  it("shows a message in #route-diagnostic and reveals it", () => {
    router.showRouteDiagnostic("heads up");
    const el = document.querySelector("#route-diagnostic");
    expect(el?.textContent).toBe("heads up");
    expect(el?.classList.contains("hidden")).toBe(false);
  });

  it("clears and re-hides #route-diagnostic", () => {
    router.showRouteDiagnostic("heads up");
    router.clearRouteDiagnostic();
    const el = document.querySelector("#route-diagnostic");
    expect(el?.textContent).toBe("");
    expect(el?.classList.contains("hidden")).toBe(true);
  });
});

describe("navigate (the commit + history choke point — never render)", () => {
  it("commits the patch to the store", () => {
    router.navigate({ lens: "list" });
    expect(store.getState().lens).toBe("list");
  });

  it("repaints only through the store subscription, never a direct render call", () => {
    // render is the single store subscriber (wired in the composition root); navigate
    // owns no render import. The sole repaint signal is the commit notification.
    let notifications = 0;
    store.subscribe(() => {
      notifications += 1;
    });
    router.navigate({ lens: "attention" });
    expect(notifications).toBe(1);
  });

  it("pushes the serialized state onto history by default", () => {
    const push = vi.spyOn(history, "pushState");
    const replace = vi.spyOn(history, "replaceState");
    router.navigate({ lens: "list" });
    expect(push).toHaveBeenCalledTimes(1);
    expect(replace).not.toHaveBeenCalled();
    expect(push.mock.calls[0]?.[2]).toBe("#/list");
    push.mockRestore();
    replace.mockRestore();
  });

  it("replaces history for a refinement", () => {
    const push = vi.spyOn(history, "pushState");
    const replace = vi.spyOn(history, "replaceState");
    router.navigate({ filterText: "needle" }, { replace: true });
    expect(replace).toHaveBeenCalledTimes(1);
    expect(push).not.toHaveBeenCalled();
    push.mockRestore();
    replace.mockRestore();
  });

  it("reconciles a closed diff through the commit invariant", () => {
    router.navigate({ diff: "obj:1", diffHash: "sha256:abc" });
    router.navigate({ diff: null });
    expect(store.getState().diffHash).toBeNull();
  });
});

describe("applyHash (derive the view from the fragment, repaint via the subscription)", () => {
  // Seed enough store state that resolve's existence checks have data to resolve
  // against. A synthetic history/revisions/objects doc keeps the test independent of
  // the captured fixtures.
  function seed(): void {
    store.commit({
      history: {
        entries: [{ eventId: EVT, eventType: "work_object_proposed" }],
        diagnostics: [],
      } as unknown as HistoryDoc,
      revisions: {
        entries: [{ revisionId: REV, snapshotId: "obj:1" }],
      } as unknown as RevisionsDoc,
      threads: {
        threads: [{ revisions: [REV] }],
        revisionClassification: {},
      } as unknown as ThreadsDoc,
    });
  }

  it("commits the resolved patch for a valid fragment and notifies the subscription", () => {
    seed();
    let notifications = 0;
    store.subscribe(() => {
      notifications += 1;
    });
    history.replaceState(null, "", `#/revision/${encodeURIComponent(REV)}`);
    router.applyHash();
    expect(store.getState().selected).toEqual({ kind: "revision", id: REV });
    expect(notifications).toBe(1);
  });

  it("fetches-to-reveal a deep link to an off-page event, then selects it", async () => {
    seed();
    mountInspectorDom();
    const X =
      "evt:sha256:0000000000000000000000000000000000000000000000000000000000000abc";
    setHistoryResponse({
      entries: [{ eventId: X, eventType: "review_observation_recorded" }],
      diagnostics: [],
      offset: 120,
      matchCount: 500,
      matchIndex: 123,
      facets: {},
    });
    history.replaceState(null, "", `#/event/${encodeURIComponent(X)}`);
    router.applyHash();
    await flush();
    expect(store.getState().selected).toEqual({ kind: "event", id: X });
    // No "not in this store" fallback — the event was fetched, not reported absent.
    const el = document.querySelector("#route-diagnostic");
    expect(el?.textContent ?? "").not.toContain("is not in this store");
  });

  it("falls back and shows a diagnostic when a deep link names a genuinely absent event", async () => {
    seed();
    mountInspectorDom();
    setHistoryResponse({
      entries: [],
      diagnostics: [],
      offset: 0,
      matchCount: 0,
      matchIndex: null,
      facets: {},
    });
    history.replaceState(null, "", "#/event/evt:sha256:absent");
    router.applyHash();
    await flush();
    expect(store.getState().selected).toEqual({ kind: null, id: null });
    const el = document.querySelector("#route-diagnostic");
    expect(el?.classList.contains("hidden")).toBe(false);
    expect(el?.textContent).toContain("is not in this store");
  });

  it("replace-rewrites a #/threads link's path only, query string byte-for-byte", async () => {
    seed();
    mountInspectorDom();
    const push = vi.spyOn(history, "pushState");
    try {
      // Include a parameter the serializer does not know, to prove the rewrite
      // preserves the original query verbatim (not serializeState output).
      history.replaceState(null, "", "#/threads?track=trk&future=1");
      router.applyHash();
      await flush();
      expect(location.hash).toBe("#/list?track=trk&future=1");
      expect(store.getState().lens).toBe("list");
      expect(store.getState().filterTrack).toBe("trk");
      // Replace, never push: Back must not bounce through the stale form.
      expect(push).not.toHaveBeenCalled();
    } finally {
      push.mockRestore();
    }
  });

  it("keeps an absent-but-in-thread revision selected on its entity-primary page", () => {
    // The revision is grouped away from the loaded list but present in a thread:
    // the entity-primary composite fetch resolves it exactly, so the selection
    // survives instead of being cleared or re-lensed.
    store.commit({
      history: { entries: [], diagnostics: [] } as unknown as HistoryDoc,
      revisions: { entries: [] } as unknown as RevisionsDoc,
      threads: {
        threads: [{ revisions: [REV] }],
        revisionClassification: {},
      } as unknown as ThreadsDoc,
    });
    mountInspectorDom();
    history.replaceState(
      null,
      "",
      `#/revision/${encodeURIComponent(REV)}?lens=list`,
    );
    router.applyHash();
    expect(store.getState().selected).toEqual({ kind: "revision", id: REV });
    expect(store.getState().open).toBe(true);
    expect(store.getState().lens).toBe("list");
  });

  it("opens an absent-but-in-thread parked cursor so the state stays visible", () => {
    // The parked form of the same deep link (e.g. a legacy #/threads?sel=…):
    // the list lens has no card for a grouped-away id, so a closed cursor would
    // be invisible state — no highlighted card, no detail, no diagnostic. The
    // selection opens as the detail page instead (the entity-primary fetch
    // resolves the exact id).
    store.commit({
      history: { entries: [], diagnostics: [] } as unknown as HistoryDoc,
      revisions: { entries: [] } as unknown as RevisionsDoc,
      threads: {
        threads: [{ revisions: [REV] }],
        revisionClassification: {},
      } as unknown as ThreadsDoc,
    });
    mountInspectorDom();
    history.replaceState(null, "", `#/threads?sel=${encodeURIComponent(REV)}`);
    router.applyHash();
    expect(store.getState().lens).toBe("list");
    expect(store.getState().selected).toEqual({ kind: "revision", id: REV });
    expect(store.getState().open).toBe(true);
  });

  it("keeps a parked cursor parked when its revision is in the loaded list", () => {
    seed();
    mountInspectorDom();
    history.replaceState(null, "", `#/list?sel=${encodeURIComponent(REV)}`);
    router.applyHash();
    expect(store.getState().selected).toEqual({ kind: "revision", id: REV });
    expect(store.getState().open).toBe(false); // the list card is the visible state
  });

  it("still clears a revision selection that is in neither the list nor any thread", () => {
    store.commit({
      history: { entries: [], diagnostics: [] } as unknown as HistoryDoc,
      revisions: { entries: [] } as unknown as RevisionsDoc,
      threads: {
        threads: [],
        revisionClassification: {},
      } as unknown as ThreadsDoc,
    });
    mountInspectorDom();
    history.replaceState(null, "", `#/revision/${encodeURIComponent(REV)}`);
    router.applyHash();
    expect(store.getState().selected).toEqual({ kind: null, id: null });
    expect(document.querySelector("#route-diagnostic")?.textContent).toContain(
      "is not in this store",
    );
  });

  it("clears the diagnostic for an unproblematic fragment", () => {
    seed();
    mountInspectorDom();
    router.showRouteDiagnostic("stale");
    history.replaceState(null, "", "#/list");
    router.applyHash();
    expect(
      document.querySelector("#route-diagnostic")?.classList.contains("hidden"),
    ).toBe(true);
  });

  it("falls back to the timeline for an unknown route", () => {
    seed();
    mountInspectorDom();
    history.replaceState(null, "", "#/bogus");
    router.applyHash();
    expect(store.getState().lens).toBe("timeline");
    expect(document.querySelector("#route-diagnostic")?.textContent).toContain(
      "unknown route",
    );
  });
});

describe("resolve surfaces the live-state diagnostic for reserved links", () => {
  it("reports an unsupported as-of link while keeping the requested view", () => {
    store.commit({
      history: { entries: [], diagnostics: [] } as unknown as HistoryDoc,
      revisions: { entries: [] } as unknown as RevisionsDoc,
      threads: {
        threads: [],
        revisionClassification: {},
      } as unknown as ThreadsDoc,
    });
    mountInspectorDom();
    const patch = router.resolve(router.parseHash("#/list?asof=2026", PT));
    expect(patch.lens).toBe("list");
    expect(document.querySelector("#route-diagnostic")?.textContent).toContain(
      "as-of links are not supported",
    );
  });
});
