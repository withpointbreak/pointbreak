import { describe, expect, it, vi } from "vitest";
import {
  type FetchFn,
  InspectClient,
  InspectClientError,
} from "../src/inspectClient";
import { VERSION_DOC } from "./fixtures";

const IDENTITY = {
  storeIdentity: "store:sha256:store",
  contextIdentity: "context:sha256:context",
};

describe("InspectClient", () => {
  it("authenticates version then identity without exposing its credentials", async () => {
    const fetch = vi
      .fn<FetchFn>()
      .mockResolvedValueOnce(response(VERSION_DOC))
      .mockResolvedValueOnce(
        response({ schema: "pointbreak.inspect-identity", ...IDENTITY }),
      );
    const client = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      fetch,
    );

    await expect(client.verify(IDENTITY)).resolves.toBeUndefined();

    expect(fetch.mock.calls.map(([url]) => url.pathname)).toEqual([
      "/api/version",
      "/api/identity",
    ]);
    for (const [url, init] of fetch.mock.calls) {
      expect(url.hash).toBe("");
      expect(init.headers).toEqual({
        Host: "127.0.0.1:63831",
        Authorization: "Bearer secret-bearer",
      });
    }
    expect(JSON.stringify(client)).not.toContain("secret-bearer");
  });

  it.each([
    [401, "unauthorized"],
    [404, "protocol"],
    [500, "protocol"],
  ] as const)("classifies HTTP %i without echoing secrets", async (status, kind) => {
    const fetch = vi.fn<FetchFn>(async () => ({
      status,
      text: async () => "secret-bearer http://127.0.0.1:63831",
    }));
    const client = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      fetch,
    );

    const error = await client.verify(IDENTITY).catch((caught) => caught);
    expect(error).toBeInstanceOf(InspectClientError);
    expect(error.kind).toBe(kind);
    expect(error.message).not.toMatch(/secret-bearer|127\.0\.0\.1|63831/);
  });

  it("rejects version and identity mismatches with typed secret-free failures", async () => {
    const incompatible = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      vi.fn<FetchFn>(async () =>
        response({ ...VERSION_DOC, cliVersion: "9.0.0" }),
      ),
    );
    await expect(incompatible.verify(IDENTITY)).rejects.toMatchObject({
      kind: "incompatible",
    });

    const mismatchFetch = vi
      .fn<FetchFn>()
      .mockResolvedValueOnce(response(VERSION_DOC))
      .mockResolvedValueOnce(
        response({
          schema: "pointbreak.inspect-identity",
          storeIdentity: IDENTITY.storeIdentity,
          contextIdentity: "context:sha256:other",
        }),
      );
    const mismatch = new InspectClient(
      "http://127.0.0.1:63831",
      "secret-bearer",
      mismatchFetch,
    );
    await expect(mismatch.verify(IDENTITY)).rejects.toMatchObject({
      kind: "mismatch",
    });
  });

  it("refuses to turn a fragment-bearing capability into an HTTP base", () => {
    expect(
      () =>
        new InspectClient(
          "http://127.0.0.1:63831/#/timeline?token=secret-bearer",
          "secret-bearer",
          vi.fn<FetchFn>(),
        ),
    ).toThrow("Pointbreak Review returned an invalid response.");
  });
});

function response(document: unknown) {
  return {
    status: 200,
    text: async () => JSON.stringify(document),
  };
}
