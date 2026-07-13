import path from "node:path";
import { describe, expect, it } from "vitest";
import { resolveBinary } from "../src/binary";

const extensionRoot = "/extension";
const bundled = path.join(extensionRoot, "bin", "darwin-arm64", "shore");
const global = "/tools/shore";

describe("resolveBinary", () => {
  it("always uses an explicit binary path", () => {
    expect(
      resolveBinary(
        {
          binaryPath: "/custom/shore",
          useGlobalCli: false,
          platform: "darwin",
          arch: "arm64",
          path: "/tools",
          exists: () => false,
        },
        extensionRoot,
      ),
    ).toEqual({ path: "/custom/shore", source: "setting" });
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
});
