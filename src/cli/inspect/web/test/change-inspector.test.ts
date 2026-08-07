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
const page = (lens: "changes" | "attention") => ({
  schema:
    lens === "changes"
      ? "pointbreak.inspect-changes-page"
      : "pointbreak.inspect-attention",
  version: lens === "changes" ? 1 : 2,
  projectionStamp: "sha256:generation",
  next: null,
  changes: [
    {
      changeId: "change:sha256:one",
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
      projectionStamp: "sha256:generation",
    },
  ],
});

beforeEach(() => {
  vi.resetModules();
  mountInspectorDom();
  history.replaceState(null, "", "/#/changes");
});
afterEach(async () => {
  const reader = await import("../src/change-inspector");
  reader.stopChangeInspector();
  vi.restoreAllMocks();
  resetDom();
});

describe("Change-first composition", () => {
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
    await Promise.resolve();
    expect(requests).toHaveLength(4);
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
      throw new Error(`unexpected ${path}`);
    }) as typeof fetch;
    const { bootstrapChangeInspector } = await import(
      "../src/change-inspector"
    );
    await bootstrapChangeInspector({ poll: false });

    location.hash =
      "#/changes/change%3Asha256%3Aone/revisions/revision%3Asha256%3Aone?artifactHash=sha256%3Aartifact";
    window.dispatchEvent(new Event("hashchange"));
    await Promise.resolve();

    expect(requests).toHaveLength(4);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Exact Revision",
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
    expect(requests).toHaveLength(requestCount + 4);
    expect(document.querySelector("#detail-body")?.textContent).toContain(
      "Exact Revision",
    );
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
});
