import { describe, expect, it, vi } from "vitest";
import type { ResolvedBinary } from "../src/binary";
import { inspectInvocation, ReviewUrlParser } from "../src/reviewTerminal";

vi.mock("vscode", () => ({
  EventEmitter: class<T> {
    readonly event = vi.fn();
    fire = vi.fn<(value?: T) => void>();
  },
  window: { createTerminal: vi.fn() },
}));

describe("inspectInvocation", () => {
  it("requests an ephemeral port without shell interpolation", () => {
    const binary: ResolvedBinary = {
      path: "/Pointbreak & Dev/$shore",
      source: "setting",
    };

    expect(inspectInvocation(binary)).toEqual({
      file: "/Pointbreak & Dev/$shore",
      args: ["inspect", "--port", "0"],
    });
    expect(inspectInvocation(binary, 63831)).toEqual({
      file: "/Pointbreak & Dev/$shore",
      args: ["inspect", "--port", "63831"],
    });
  });
});

describe("ReviewUrlParser", () => {
  it("extracts the announced loopback URL from inspector output", () => {
    const parser = new ReviewUrlParser();

    expect(
      parser.push(`Pointbreak Review inspector
  store: .
  url:   http://127.0.0.1:63831/
  stop:  Ctrl-C
`),
    ).toBe("http://127.0.0.1:63831");
  });

  it("handles an announced URL split across output chunks", () => {
    const parser = new ReviewUrlParser();

    expect(
      parser.push("Pointbreak Review inspector\n  url: http://127.0."),
    ).toBeUndefined();
    expect(parser.push("0.1:63831/\n  stop: Ctrl-C\n")).toBe(
      "http://127.0.0.1:63831",
    );
  });

  it("ignores non-loopback URLs", () => {
    const parser = new ReviewUrlParser();

    expect(parser.push("  url: http://example.com:63831/\n")).toBeUndefined();
  });
});
