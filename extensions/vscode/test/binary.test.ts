import path from "node:path";
import { describe, expect, it } from "vitest";
import { resolveBinary } from "../src/binary";

const extensionRoot = "/extension";
const bundled = path.posix.join(
  extensionRoot,
  "bin",
  "darwin-arm64",
  "pointbreak",
);
const global = "/tools/pointbreak";

describe("resolveBinary", () => {
  it("always uses an explicit binary path", () => {
    expect(
      resolveBinary(
        {
          binaryPath: "/custom/arbitrarily-named-review-cli",
          useGlobalCli: false,
          platform: "darwin",
          arch: "arm64",
          path: "/tools",
          exists: () => false,
        },
        extensionRoot,
      ),
    ).toEqual({
      path: "/custom/arbitrarily-named-review-cli",
      source: "setting",
    });
  });

  it("prefers bundled when global CLI use is disabled", () => {
    expect(
      resolveBinary(
        {
          useGlobalCli: false,
          platform: "darwin",
          arch: "arm64",
          path: "/tools",
          exists: () => true,
        },
        extensionRoot,
      ),
    ).toEqual({ path: bundled, source: "bundled" });
  });

  it("falls back to PATH with an announcement", () => {
    const announcements: string[] = [];
    const result = resolveBinary(
      {
        useGlobalCli: false,
        platform: "darwin",
        arch: "arm64",
        path: "/tools",
        exists: (candidate) => candidate === global,
        announceFallback: (message) => announcements.push(message),
      },
      extensionRoot,
    );

    expect(result).toEqual({ path: global, source: "path" });
    expect(announcements).toHaveLength(1);
  });

  it("prefers PATH when global CLI use is enabled", () => {
    expect(
      resolveBinary(
        {
          useGlobalCli: true,
          platform: "darwin",
          arch: "arm64",
          path: "/tools",
          exists: () => true,
        },
        extensionRoot,
      ),
    ).toEqual({ path: global, source: "path" });
  });

  it("falls back to bundled with an announcement", () => {
    const announcements: string[] = [];
    const result = resolveBinary(
      {
        useGlobalCli: true,
        platform: "darwin",
        arch: "arm64",
        path: "/tools",
        exists: (candidate) => candidate === bundled,
        announceFallback: (message) => announcements.push(message),
      },
      extensionRoot,
    );

    expect(result).toEqual({ path: bundled, source: "bundled" });
    expect(announcements).toHaveLength(1);
  });

  it("returns an actionable error when no candidate exists", () => {
    expect(() =>
      resolveBinary(
        {
          useGlobalCli: false,
          platform: "darwin",
          arch: "arm64",
          path: "/tools",
          exists: () => false,
        },
        extensionRoot,
      ),
    ).toThrow(/install pointbreak|binaryPath/i);
  });

  it("ignores the retired executable in the bundle and on PATH", () => {
    const retired = new Set([
      path.posix.join(extensionRoot, "bin", "darwin-arm64", "shore"),
      "/tools/shore",
    ]);

    expect(() =>
      resolveBinary(
        {
          useGlobalCli: false,
          platform: "darwin",
          arch: "arm64",
          path: "/tools",
          exists: (candidate) => retired.has(candidate),
        },
        extensionRoot,
      ),
    ).toThrow(/pointbreak/i);
  });
});
