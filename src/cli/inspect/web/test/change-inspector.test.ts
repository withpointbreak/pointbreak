import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CHANGE_READER_DOCUMENTS } from "../src/change-protocol";
import { mountInspectorDom, resetDom } from "./support/dom";

const profile = {
  schema: "pointbreak.inspect-reader-profile",
  version: 1,
  availability: "ready",
  authorityCursor: { eventCount: 1 },
  commitGraphStamp: "sha256:stamp",
  minimumReaderProfile: "review_change_revision_v1",
  documents: { ...CHANGE_READER_DOCUMENTS },
};
const page = (
  lens: "changes" | "attention",
  projectionStamp = "sha256:generation",
) => ({
  schema:
    lens === "changes"
      ? "pointbreak.inspect-changes-page"
      : "pointbreak.inspect-attention",
  version: lens === "changes" ? 1 : 2,
  projectionStamp,
  next: null,
  changes: [
    {
      changeId: "change:sha256:one",
      declarationState: "authoritative",
      titleAssertions: [],
      memberCount: 1,
      topology: "initial",
      lifecycle: "in_progress",
      attentionSummary: "in_progress",
      availabilitySummary: "available",
      currentRevisionRefs: [
        {
          revisionId: "revision:sha256:one",
          objectArtifactContentHash: "sha256:artifact",
        },
      ],
      projectionStamp,
    },
  ],
});

const revision = {
  revisionId: "revision:sha256:one",
  objectArtifactContentHash: "sha256:artifact",
};

function revisionDetail(projectionStamp = "sha256:generation") {
  return {
    schema: "pointbreak.review-change-revision",
    version: 1,
    changeId: "change:sha256:one",
    revision,
    membershipSupport: [],
    revisionCurrency: "current",
    relationClassification: "current",
    availability: "available",
    exactRevisionDocument: {
      schema: "pointbreak.review-revision-resource",
      version: 1,
      projectionStamp,
      resource: { revision, objectId: "obj:sha256:one" },
      projection: { includeBody: true },
      availability: "available",
      capturedDocumentHash: "sha256:captured",
      capturedDocument: {
        schema: "pointbreak.review-snapshot",
        version: 1,
        contentHash: revision.objectArtifactContentHash,
        snapshot: {
          review_id: "review:sha256:one",
          object_id: "obj:sha256:one",
          files: [],
        },
      },
      diagnostics: [],
      cacheKey: "sha256:resource",
    },
    factPresentations: [],
    factPorts: [],
    associations: [],
    diagnostics: [],
    projectionStamp,
  };
}

function staleProjectionResponse(): Response {
  return new Response(
    JSON.stringify({
      schema: "pointbreak.inspect-change-page-error",
      version: 1,
      code: "stale_projection",
    }),
    { status: 409 },
  );
}

function isExactRevisionPath(path: string): boolean {
  return path.startsWith(
    "/api/v2/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?",
  );
}

function isExactResourcePath(path: string): boolean {
  return path.startsWith(
    "/api/v2/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone/resource?",
  );
}

beforeEach(() => {
  vi.resetModules();
  localStorage.clear();
  mountInspectorDom();
  history.replaceState(null, "", "/#/changes");
});
afterEach(async () => {
  const reader = await import("../src/change-inspector");
  reader.stopChangeInspector();
  vi.useRealTimers();
  vi.restoreAllMocks();
  resetDom();
});

describe("Change-first composition", () => {
  it("consumes a same-document capability before strict Change routing", async () => {
    const token = "opaque_test_capability_0123456789abcdef";
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    const { sessionTokenKey } = await import("../src/auth");
    await bootstrapChangeInspector({ poll: false });
    requests.length = 0;

    history.replaceState(
      null,
      "",
      `/#/changes?limit=100&order=change_id_asc&token=${token}`,
    );
    window.dispatchEvent(new HashChangeEvent("hashchange"));

    await vi.waitFor(() =>
      expect(location.hash).toBe("#/changes?limit=100&order=change_id_asc"),
    );
    expect(sessionStorage.getItem(sessionTokenKey())).toBe(token);
    await vi.waitFor(() =>
      expect(requests).toContain(
        "/api/v2/changes?limit=100&order=change_id_asc",
      ),
    );
    expect(document.querySelector("#route-diagnostic")?.textContent).toBe("");
  });

  it("keeps keyboard selection local until Enter and leaves text entry alone", async () => {
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true }),
    );
    expect(document.querySelector(".change-card-selected")).not.toBeNull();
    expect(location.hash).toBe("#/changes");
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    await vi.waitFor(() =>
      expect(location.hash).toContain("/changes/change%3Asha256%3Aone"),
    );

    const search = document.querySelector<HTMLInputElement>("#filter-text");
    search?.focus();
    search?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "j", bubbles: true }),
    );
    expect(document.querySelector(".change-card-selected")).not.toBeNull();
    search?.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "k",
        ctrlKey: true,
        bubbles: true,
      }),
    );
    expect(document.querySelector("#cmd-palette")?.classList).toContain(
      "hidden",
    );
  });

  it("makes the closed narrow detail inert and restores focus after an exact reading", async () => {
    vi.spyOn(window, "matchMedia").mockImplementation(
      (query: string) =>
        ({
          matches: query === "(max-width: 760px)",
          media: query,
          onchange: null,
          addListener: vi.fn(),
          removeListener: vi.fn(),
          addEventListener: vi.fn(),
          removeEventListener: vi.fn(),
          dispatchEvent: vi.fn(() => true),
        }) as unknown as MediaQueryList,
    );
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    const detail = document.querySelector<HTMLElement>("#detail");
    const opener = document.querySelector<HTMLButtonElement>(
      "[data-change-id] .change-card-peer-open",
    );
    expect(detail?.inert).toBe(true);
    expect(detail?.getAttribute("aria-hidden")).toBe("true");
    opener?.focus();
    opener?.click();
    await vi.waitFor(() =>
      expect(
        document.querySelector<HTMLElement>("#detail-body")?.dataset
          .changeReadingKey,
      ).toBeDefined(),
    );
    expect(detail?.inert).toBe(false);
    expect(detail?.hasAttribute("aria-hidden")).toBe(false);
    expect(document.activeElement).toBe(document.querySelector("#detail-back"));

    document.querySelector<HTMLButtonElement>("#detail-back")?.click();
    await vi.waitFor(() => {
      expect(location.hash).toBe("#/changes");
      expect(detail?.inert).toBe(true);
    });
    expect(detail?.inert).toBe(true);
    expect(detail?.getAttribute("aria-hidden")).toBe("true");
    expect(document.activeElement).toBe(opener);
  });

  it("preserves the bounded query across lens changes and exposes local display modes", async () => {
    history.replaceState(null, "", "/#/changes?q=needle");
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    document
      .querySelector<HTMLButtonElement>("[data-lens='attention']")
      ?.click();
    expect(location.hash).toContain("#/attention?q=needle");
    expect(document.querySelector("#view-order-section")?.classList).toContain(
      "hidden",
    );
    expect(
      document.querySelector("#jump-latest")?.closest(".control-section")
        ?.classList,
    ).toContain("hidden");
    const compact =
      document.querySelector<HTMLInputElement>("#density-compact");
    if (compact) {
      compact.checked = true;
      compact.dispatchEvent(new Event("change", { bubbles: true }));
    }
    expect(document.documentElement.classList.contains("compact")).toBe(true);

    history.replaceState(
      null,
      "",
      "/#/attention?q=needle&topology=initial&after=opaque&limit=20&order=change_id_asc",
    );
    document.querySelector<HTMLButtonElement>("#filter-clear")?.click();
    expect(location.hash).toBe("#/attention?limit=20&order=change_id_asc");
  });

  it("sends an opaque continuation only to its active lens and clears it on lens changes", async () => {
    history.replaceState(
      null,
      "",
      "/#/changes?after=changes-page&limit=20&order=change_id_asc",
    );
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    expect(requests).toContain(
      "/api/v2/changes?limit=20&after=changes-page&order=change_id_asc",
    );
    expect(requests).toContain(
      "/api/v2/attention?limit=20&order=change_id_asc",
    );

    document
      .querySelector<HTMLButtonElement>("[data-lens='attention']")
      ?.click();
    expect(location.hash).toBe("#/attention?limit=20&order=change_id_asc");
    await vi.waitFor(() => {
      expect(
        requests.filter((path) => path.startsWith("/api/v2/changes?")),
      ).toHaveLength(2);
      expect(
        requests.filter((path) => path.startsWith("/api/v2/attention?")),
      ).toHaveLength(2);
    });
    expect(requests.at(-3)).toBe(
      "/api/v2/changes?limit=20&order=change_id_asc",
    );
    expect(requests.at(-2)).toBe(
      "/api/v2/attention?limit=20&order=change_id_asc",
    );
  });

  it("drops an Attention continuation before exact navigation so later polls cannot cross lenses", async () => {
    history.replaceState(
      null,
      "",
      "/#/attention?after=attention-page&limit=20&order=change_id_asc",
    );
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    document
      .querySelector<HTMLButtonElement>(".change-card-peer-open")
      ?.click();
    await vi.waitFor(() =>
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Exact Revision",
      ),
    );
    expect(location.hash).not.toContain("after=");

    pollTick();
    await vi.waitFor(() =>
      expect(
        requests.filter((path) => path.startsWith("/api/v2/profile")).length,
      ).toBeGreaterThanOrEqual(6),
    );
    expect(
      requests.some(
        (path) =>
          path.startsWith("/api/v2/changes?") &&
          path.includes("after=attention-page"),
      ),
    ).toBe(false);
  });

  it("maps only 1 and 2 to Change lenses and keeps split and reading controls local", async () => {
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "2", bubbles: true }),
    );
    expect(location.hash).toBe("#/attention");
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "3", bubbles: true }),
    );
    expect(location.hash).toBe("#/attention");
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "1", bubbles: true }),
    );
    expect(location.hash).toBe("#/changes");

    const divider = document.querySelector<HTMLElement>(".divider");
    divider?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
    );
    expect(divider?.getAttribute("aria-valuenow")).toBe("55");
    for (let step = 0; step < 6; step += 1) {
      divider?.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "ArrowRight",
          bubbles: true,
        }),
      );
    }
    expect(divider?.getAttribute("aria-valuenow")).toBe("75");
    divider?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    expect(divider?.getAttribute("aria-valuenow")).toBe("50");

    const detailViewport = document.querySelector<HTMLElement>("#detail-body");
    if (detailViewport) detailViewport.scrollTop = 17;
    const reading = document.querySelector<HTMLButtonElement>("#detail-read");
    reading?.click();
    expect(
      document.querySelector(".split")?.classList.contains("reading"),
    ).toBe(true);
    expect(reading?.getAttribute("aria-label")).toBe("Exit reading mode");
    expect(detailViewport?.scrollTop).toBe(17);
    document.querySelector<HTMLButtonElement>("#master-rail")?.click();
    expect(
      document.querySelector(".split")?.classList.contains("reading"),
    ).toBe(false);

    document.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "P",
        ctrlKey: true,
        shiftKey: true,
        bubbles: true,
      }),
    );
    expect(document.querySelector("#cmd-palette")?.classList).not.toContain(
      "hidden",
    );
    const paletteInput = document.querySelector<HTMLInputElement>("#cmd-input");
    expect(document.activeElement).toBe(paletteInput);
    expect(paletteInput?.getAttribute("role")).toBeNull();
    expect(
      document.querySelector("#cmd-results")?.getAttribute("role"),
    ).toBeNull();
    if (paletteInput) {
      paletteInput.value = "attention";
      paletteInput.dispatchEvent(new Event("input", { bubbles: true }));
    }
    expect(
      Array.from(
        document.querySelectorAll<HTMLButtonElement>("#cmd-results button"),
      ).map((button) => button.textContent),
    ).toEqual(["Open Attention"]);
    if (paletteInput) {
      paletteInput.value = "";
      paletteInput.dispatchEvent(new Event("input", { bubbles: true }));
    }
    const paletteButtons = Array.from(
      document.querySelectorAll<HTMLButtonElement>("#cmd-results button"),
    );
    paletteInput?.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Tab",
        shiftKey: true,
        bubbles: true,
      }),
    );
    expect(document.activeElement).toBe(paletteButtons.at(-1));
    paletteButtons
      .at(-1)
      ?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Tab", bubbles: true }),
      );
    expect(document.activeElement).toBe(paletteInput);
    paletteInput?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
    expect(document.querySelector("#cmd-palette")?.classList).toContain(
      "hidden",
    );

    const firstCard = document.querySelector<HTMLElement>(
      ".unit-card[data-change-id]",
    );
    const lastCard = firstCard?.cloneNode(true) as HTMLElement | undefined;
    if (lastCard) {
      lastCard.dataset.changeId = "change:sha256:last";
      firstCard?.parentElement?.append(lastCard);
    }
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "G", bubbles: true }),
    );
    expect(
      document.querySelector<HTMLElement>(".change-card-selected")?.dataset
        .changeId,
    ).toBe("change:sha256:last");
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "g", bubbles: true }),
    );
    expect(
      document.querySelector<HTMLElement>(".change-card-selected")?.dataset
        .changeId,
    ).toBe("change:sha256:one");
  });

  it("validates profile before staging the two bounded lenses and does not fetch placeholder detail", async () => {
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    expect(requests).toEqual([
      "/api/v2/profile",
      "/api/v2/changes?limit=50&order=change_id_asc",
      "/api/v2/attention?limit=50&order=change_id_asc",
      "/api/v2/profile",
    ]);
    document
      .querySelector<HTMLButtonElement>(
        "[data-change-id] .change-card-peer-open",
      )
      ?.click();
    await vi.waitFor(() => expect(requests).toHaveLength(6));
    expect(location.hash).toContain("artifactHash=sha256%3Aartifact");
  });

  it("refuses an invalid route without fetching any semantic document", async () => {
    history.replaceState(null, "", "/#/changes?unknown=value");
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      requests.push(String(input));
      throw new Error("invalid routes must not fetch");
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );

    await bootstrapChangeInspector({ poll: false });

    expect(requests).toEqual([]);
    expect(document.querySelector("#route-diagnostic")?.textContent).toContain(
      "Unknown unknown route query.",
    );
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Unknown unknown route query.",
    );
  });

  it("clears the old generation for a new query while its profile-first replacement is pending", async () => {
    let replacementProfileResolve!: (value: Response) => void;
    let profileRequests = 0;
    const requests: string[] = [];
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (profileRequests === 3) {
          return new Promise<Response>((resolve) => {
            replacementProfileResolve = resolve;
          });
        }
        return Promise.resolve(new Response(JSON.stringify(profile)));
      }
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (isExactRevisionPath(path))
        return Promise.resolve(new Response(JSON.stringify(revisionDetail())));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });
    expect(document.querySelector("#master")?.textContent).toContain(
      "change:sha256:one",
    );

    location.hash = "#/changes?q=replacement";
    window.dispatchEvent(new Event("hashchange"));
    await vi.waitFor(() => {
      expect(document.querySelector("#master")?.textContent).toContain(
        "Loading Change generation",
      );
    });
    expect(requests).toHaveLength(5);
    expect(requests.slice(4)).toEqual(["/api/v2/profile"]);

    replacementProfileResolve(new Response(JSON.stringify(profile)));
    await vi.waitFor(() => {
      expect(requests).toContain(
        "/api/v2/changes?limit=50&q=replacement&order=change_id_asc",
      );
    });
  });

  it("reuses a coherent generation for exact navigation with the same query", async () => {
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    location.hash =
      "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact";
    window.dispatchEvent(new Event("hashchange"));
    await vi.waitFor(() => expect(requests).toHaveLength(6));

    expect(requests).toHaveLength(6);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Exact Revision",
    );
  });

  it("rehydrates the same exact route when polling publishes a newer projection", async () => {
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let generation = 1;
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      const stamp = `sha256:generation-${generation}`;
      if (path === "/api/v2/profile")
        return new Response(
          JSON.stringify({
            ...profile,
            authorityCursor: { eventCount: generation },
          }),
        );
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes", stamp)));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention", stamp)));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail(stamp)));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();
    expect(requests.filter((path) => isExactRevisionPath(path))).toHaveLength(
      1,
    );

    generation = 2;
    pollTick();
    await vi.waitFor(() =>
      expect(requests.filter((path) => isExactRevisionPath(path))).toHaveLength(
        2,
      ),
    );
    expect(
      document.querySelector<HTMLElement>("#detail-body")?.dataset
        .changeReadingKey,
    ).toContain("sha256:generation-2");
  });

  it("coalesces overlapping ticks behind one slow generation poll", async () => {
    vi.useFakeTimers();
    let changesRequests = 0;
    let activeProjectionStamp = "sha256:generation-0";
    let resolveSlowChanges!: (response: Response) => void;
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return Promise.resolve(new Response(JSON.stringify(profile)));
      if (path.startsWith("/api/v2/changes?")) {
        changesRequests += 1;
        activeProjectionStamp = `sha256:generation-${changesRequests}`;
        if (changesRequests === 2) {
          return new Promise<Response>((resolve) => {
            resolveSlowChanges = resolve;
          });
        }
        return Promise.resolve(
          new Response(JSON.stringify(page("changes", activeProjectionStamp))),
        );
      }
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(
          new Response(
            JSON.stringify(page("attention", activeProjectionStamp)),
          ),
        );
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    await vi.advanceTimersByTimeAsync(3_000);
    expect(changesRequests).toBe(2);
    await vi.advanceTimersByTimeAsync(6_000);
    expect(changesRequests).toBe(2);

    resolveSlowChanges(
      new Response(JSON.stringify(page("changes", "sha256:generation-2"))),
    );
    await vi.waitFor(() => {
      expect(changesRequests).toBe(3);
      expect(document.querySelector("#stat-hash")?.textContent).toBe(
        "sha256:generation-3",
      );
    });
  });

  it("times out a hung exact postflight and releases its coalesced successor", async () => {
    vi.useFakeTimers();
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let generation = 1;
    let profileRequests = 0;
    let changesRequests = 0;
    let exactRequests = 0;
    let markHungPostflightStarted!: () => void;
    const hungPostflightStarted = new Promise<void>((resolve) => {
      markHungPostflightStarted = resolve;
    });
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      const stamp = `sha256:generation-${generation}`;
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (profileRequests === 6) {
          markHungPostflightStarted();
          return new Promise<Response>(() => {});
        }
        return Promise.resolve(
          new Response(
            JSON.stringify({
              ...profile,
              authorityCursor: { eventCount: generation },
            }),
          ),
        );
      }
      if (path.startsWith("/api/v2/changes?")) {
        changesRequests += 1;
        return Promise.resolve(
          new Response(JSON.stringify(page("changes", stamp))),
        );
      }
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(
          new Response(JSON.stringify(page("attention", stamp))),
        );
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        return Promise.resolve(
          new Response(JSON.stringify(revisionDetail(stamp))),
        );
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    generation = 2;
    pollTick();
    await hungPostflightStarted;
    expect(changesRequests).toBe(2);
    expect(exactRequests).toBe(2);
    pollTick();
    pollTick();
    expect(changesRequests).toBe(2);
    expect(exactRequests).toBe(2);

    const detail = document.querySelector<HTMLElement>("#detail-body");
    if (detail === null) throw new Error("missing exact detail body");
    const repainted = new Promise<void>((resolve) => {
      const observer = new MutationObserver(() => {
        if (detail.dataset.changeReadingKey?.includes("sha256:generation-2")) {
          observer.disconnect();
          resolve();
        }
      });
      observer.observe(detail, {
        attributes: true,
        attributeFilter: ["data-change-reading-key"],
      });
    });
    await vi.advanceTimersByTimeAsync(10_000);
    await repainted;
    expect(changesRequests).toBe(3);
    expect(exactRequests).toBe(3);
    expect(
      document.querySelector<HTMLElement>("#detail-body")?.dataset
        .changeReadingKey,
    ).toContain("sha256:generation-2");
  });

  it("preserves a focused uncommitted search draft across poll paints", async () => {
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
    let generation = 1;
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      const stamp = `sha256:generation-${generation}`;
      if (path === "/api/v2/profile")
        return new Response(
          JSON.stringify({
            ...profile,
            authorityCursor: { eventCount: generation },
          }),
        );
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes", stamp)));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention", stamp)));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    const search = document.querySelector<HTMLInputElement>("#filter-text");
    if (search === null) throw new Error("missing Change search input");
    search.focus();
    search.value = "uncommitted draft";
    search.setSelectionRange(4, 11, "forward");
    expect(document.activeElement).toBe(search);

    const statHash = document.querySelector<HTMLElement>("#stat-hash");
    if (statHash === null) throw new Error("missing projection stamp status");
    const repainted = new Promise<void>((resolve) => {
      const observer = new MutationObserver(() => {
        if (statHash.textContent === "sha256:generation-2") {
          observer.disconnect();
          resolve();
        }
      });
      observer.observe(statHash, {
        childList: true,
        characterData: true,
        subtree: true,
      });
    });
    generation = 2;
    pollTick();
    await repainted;

    expect(location.hash).toBe("#/changes");
    expect(search.value).toBe("uncommitted draft");
    expect(document.activeElement).toBe(search);
    expect(search.selectionStart).toBe(4);
    expect(search.selectionEnd).toBe(11);
    expect(search.selectionDirection).toBe("forward");
  });

  it("preserves a search draft started after a background poll begins", async () => {
    let pollTick: () => void = () => {
      throw new Error("poll interval was not installed");
    };
    vi.spyOn(globalThis, "setInterval").mockImplementation((handler, delay) => {
      if (delay === 3000 && typeof handler === "function") pollTick = handler;
      return 1 as unknown as ReturnType<typeof setInterval>;
    });
    let generation = 1;
    let profileRequests = 0;
    let markPollStarted!: () => void;
    let releasePoll!: () => void;
    const pollStarted = new Promise<void>((resolve) => {
      markPollStarted = resolve;
    });
    const pollGate = new Promise<void>((resolve) => {
      releasePoll = resolve;
    });
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      const stamp = `sha256:generation-${generation}`;
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        if (profileRequests === 3) {
          markPollStarted();
          await pollGate;
        }
        return new Response(
          JSON.stringify({
            ...profile,
            authorityCursor: { eventCount: generation },
          }),
        );
      }
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes", stamp)));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention", stamp)));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector();

    const search = document.querySelector<HTMLInputElement>("#filter-text");
    if (search === null) throw new Error("missing Change search input");
    pollTick();
    await pollStarted;
    search.focus();
    search.value = "draft started during poll";
    search.setSelectionRange(6, 13, "backward");
    generation = 2;
    releasePoll();

    await vi.waitFor(() =>
      expect(document.querySelector("#stat-hash")?.textContent).toBe(
        "sha256:generation-2",
      ),
    );
    expect(location.hash).toBe("#/changes");
    expect(search.value).toBe("draft started during poll");
    expect(document.activeElement).toBe(search);
    expect(search.selectionStart).toBe(6);
    expect(search.selectionEnd).toBe(13);
    expect(search.selectionDirection).toBe("backward");
  });

  it("does not let an older same-query detail failure restart a route the user left", async () => {
    let rejectOldDetail!: (reason?: unknown) => void;
    const requests: string[] = [];
    globalThis.fetch = vi.fn((input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return Promise.resolve(new Response(JSON.stringify(profile)));
      if (path.startsWith("/api/v2/changes?"))
        return Promise.resolve(new Response(JSON.stringify(page("changes"))));
      if (path.startsWith("/api/v2/attention?"))
        return Promise.resolve(new Response(JSON.stringify(page("attention"))));
      if (isExactRevisionPath(path)) {
        return new Promise<Response>((_resolve, reject) => {
          rejectOldDetail = reject;
        });
      }
      if (isExactResourcePath(path))
        return Promise.resolve(
          new Response(JSON.stringify(revisionDetail().exactRevisionDocument)),
        );
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    location.hash =
      "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact";
    window.dispatchEvent(new Event("hashchange"));
    await vi.waitFor(() => expect(requests).toHaveLength(5));

    location.hash =
      "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone/resource?artifactHash=sha256%3Aartifact";
    window.dispatchEvent(new Event("hashchange"));
    await vi.waitFor(() => {
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Authoritative captured diff",
      );
    });
    rejectOldDetail(new Error("old detail request failed"));
    await Promise.resolve();
    await Promise.resolve();

    expect(
      requests.filter((path) => path.startsWith("/api/v2/changes?")).length,
    ).toBe(1);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Authoritative captured diff",
    );
  });

  it("restores exact selection through Back, Forward, and a fresh bootstrap", async () => {
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const reader = await import("../src/change-inspector");
    await reader.bootstrapChangeInspector({ poll: false });
    const exactHash =
      "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact";

    location.hash = exactHash;
    await vi.waitFor(() => {
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Exact Revision",
      );
    });
    location.hash = "#/attention";
    await vi.waitFor(() => {
      expect(document.querySelector("#master h1")?.textContent).toContain(
        "Attention",
      );
    });

    history.back();
    await vi.waitFor(() => {
      expect(location.hash).toBe(exactHash);
      expect(document.querySelector("#detail-body")?.textContent).toContain(
        "Exact Revision",
      );
    });
    history.forward();
    await vi.waitFor(() => {
      expect(location.hash).toBe("#/attention");
      expect(document.querySelector("#master h1")?.textContent).toContain(
        "Attention",
      );
    });
    history.back();
    await vi.waitFor(() => expect(location.hash).toBe(exactHash));

    const requestCount = requests.length;
    reader.stopChangeInspector();
    await reader.bootstrapChangeInspector({ poll: false });
    expect(requests).toHaveLength(requestCount + 6);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Exact Revision",
    );
    const viewToggle =
      document.querySelector<HTMLButtonElement>("#view-toggle");
    const viewPanel = document.querySelector("#view-panel");
    viewToggle?.click();
    expect(viewPanel?.classList).not.toContain("hidden");
    viewToggle?.click();
    expect(viewPanel?.classList).toContain("hidden");
  });

  it("dismisses lightweight control disclosures when route intent changes", async () => {
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    const viewToggle =
      document.querySelector<HTMLButtonElement>("#view-toggle");
    const viewPanel = document.querySelector("#view-panel");
    viewToggle?.click();
    expect(viewPanel?.classList).not.toContain("hidden");

    location.hash = "#/attention";
    window.dispatchEvent(new Event("hashchange"));
    await vi.waitFor(() =>
      expect(document.querySelector("#master h1")?.textContent).toContain(
        "Attention",
      ),
    );
    expect(viewPanel?.classList).toContain("hidden");
    expect(viewToggle?.getAttribute("aria-expanded")).toBe("false");

    const filtersToggle =
      document.querySelector<HTMLButtonElement>("#filters-toggle");
    const filtersPanel = document.querySelector("#filters-panel");
    filtersToggle?.click();
    expect(filtersPanel?.classList).not.toContain("hidden");

    location.hash = "#/changes";
    window.dispatchEvent(new Event("hashchange"));
    await vi.waitFor(() =>
      expect(document.querySelector("#master h1")?.textContent).toContain(
        "Changes",
      ),
    );
    expect(filtersPanel?.classList).toContain("hidden");
    expect(filtersToggle?.getAttribute("aria-expanded")).toBe("false");
  });

  it("retries one profile-generation mismatch exactly once", async () => {
    let profileRequests = 0;
    const requests: string[] = [];
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      requests.push(path);
      if (path === "/api/v2/profile") {
        profileRequests += 1;
        const eventCount = profileRequests === 2 ? 2 : 1;
        return new Response(
          JSON.stringify({
            ...profile,
            authorityCursor: { eventCount },
          }),
        );
      }
      if (path.startsWith("/api/v2/changes?"))
        return new Response(JSON.stringify(page("changes")));
      if (path.startsWith("/api/v2/attention?"))
        return new Response(JSON.stringify(page("attention")));
      if (isExactRevisionPath(path))
        return new Response(JSON.stringify(revisionDetail()));
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );

    await bootstrapChangeInspector({ poll: false });

    expect(profileRequests).toBe(4);
    expect(
      requests.filter((path) => path.startsWith("/api/v2/changes?")).length,
    ).toBe(2);
    expect(
      requests.filter((path) => path.startsWith("/api/v2/attention?")).length,
    ).toBe(2);
    expect(document.querySelector("#master")?.textContent).toContain(
      "change:sha256:one",
    );
  });

  it("shares one projection restart across generation and exact-detail reads", async () => {
    history.replaceState(
      null,
      "",
      "/#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact",
    );
    let changesRequests = 0;
    let attentionRequests = 0;
    let exactRequests = 0;
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path === "/api/v2/profile")
        return new Response(JSON.stringify(profile));
      if (path.startsWith("/api/v2/changes?")) {
        changesRequests += 1;
        return changesRequests === 1
          ? staleProjectionResponse()
          : new Response(JSON.stringify(page("changes")));
      }
      if (path.startsWith("/api/v2/attention?")) {
        attentionRequests += 1;
        return new Response(JSON.stringify(page("attention")));
      }
      if (isExactRevisionPath(path)) {
        exactRequests += 1;
        return staleProjectionResponse();
      }
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );

    await bootstrapChangeInspector({ poll: false });

    expect(changesRequests).toBe(2);
    expect(attentionRequests).toBe(2);
    expect(exactRequests).toBe(1);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "server response error",
    );
  });
});
