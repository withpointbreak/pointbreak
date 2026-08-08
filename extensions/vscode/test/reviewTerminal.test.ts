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

const TOKEN =
  "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-";

describe("inspectInvocation", () => {
  it("starts only an ephemeral text-web server with sanitized spawn arguments", () => {
    const binary: ResolvedBinary = {
      path: "/Pointbreak & Dev/$pointbreak",
      source: "setting",
    };

    expect(
      inspectInvocation(binary, {
        POINTBREAK_ACTOR_ID: "actor:agent:leaked",
        POINTBREAK_FORMAT: "text",
        SAFE_INPUT: "preserved",
      }),
    ).toEqual({
      file: "/Pointbreak & Dev/$pointbreak",
      args: ["inspect", "--port", "0"],
      env: { SAFE_INPUT: "preserved" },
    });
  });
});

describe("ReviewUrlParser", () => {
  it("splits a fragment capability into a secret-free origin and private bearer", () => {
    const parser = new ReviewUrlParser();

    expect(
      parser.push(`Pointbreak Review inspector
  store: .
  url:   http://127.0.0.1:63831/#/changes?token=${TOKEN}
  stop:  Ctrl-C
`),
    ).toEqual({ origin: "http://127.0.0.1:63831", token: TOKEN });
  });

  it("handles a capability split across chunks", () => {
    const parser = new ReviewUrlParser();

    expect(
      parser.push("Pointbreak Review inspector\n  url: http://127.0."),
    ).toBeUndefined();
    expect(parser.push(`0.1:63831/#/changes?token=${TOKEN}\n`)).toEqual({
      origin: "http://127.0.0.1:63831",
      token: TOKEN,
    });
  });

  it.each([
    `http://example.com:63831/#/timeline?token=${TOKEN}`,
    `http://localhost:63831/#/timeline?token=${TOKEN}`,
    "http://127.0.0.1:63831/#/timeline",
    "http://127.0.0.1:63831/#/timeline?token=short",
  ])("ignores an invalid or non-IP-loopback capability", (url) => {
    const parser = new ReviewUrlParser();
    expect(parser.push(`  url: ${url}\n`)).toBeUndefined();
  });
});
